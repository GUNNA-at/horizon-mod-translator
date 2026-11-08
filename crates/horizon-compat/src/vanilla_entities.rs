//! Vanilla's entities as translated mods meet them, natively: the mob a player hits or
//! right-clicks, the minecart a wrench breaks, the one hurt or killed. Each is an object of its
//! 1.21.1 class (`Cow`, `Minecart`, `Zombie`) standing for the game's entity by its id, the same
//! object while the game keeps the entity, so `instanceof Animal`, `AbstractMinecart` or `Enemy`
//! answer as in Java ([`is_a`], asked by `api_common::api_is_a`) and a mod may key a map by it.
//! It reads the entity's type, place, facing and health from the game ([`Host::entity`]) and acts
//! through it: `hurt`, `kill`, `discard`, `setHealth`, `push` and `setDeltaMovement`,
//! `teleportTo`. A player is the runtime's server player ([`crate::menus::server_player`]), an
//! item entity `entities.rs`' object.
//!
//! The tables are 1.21.1's names and sizes (from its `EntityType` fields' generic types, the
//! `Builder.sized` and `eyeHeight` calls, and the classes' own superclasses and interfaces). A
//! 26.3 type 1.21.1 lacks is a boat by its name (`oak_boat`), else a `PathfinderMob` when the
//! game gives it health, else an `Entity`.
//!
//! Not modelled yet: a mob's motion, pitch, equipment and effects (none), its UUID (a stable one
//! made from the id, as `lan::entity_uuid` makes for entities with none), and a blow's armour,
//! knockback and attacker: `hurt` takes the amount off the health.

use crate::host::{EntityView, Host, Nbt};
use crate::{Rt, V, api};

const ENTITY: &str = "net/minecraft/world/entity/Entity";
const LIVING: &str = "net/minecraft/world/entity/LivingEntity";
const PATHFINDER: &str = "net/minecraft/world/entity/PathfinderMob";
const BOAT: &str = "net/minecraft/world/entity/vehicle/Boat";
const CHEST_BOAT: &str = "net/minecraft/world/entity/vehicle/ChestBoat";
/// The objects made so far, in a list the intern table keeps.
const LIST: &str = "compat:vanilla-entities";
/// Ticks an object is kept once nothing has asked for its entity and the game no longer has it.
const FORGET_AFTER: i64 = 1200;

/// 1.21.1's entity types, by id: the class each makes (`EntityType.COW` is an `EntityType<Cow>`),
/// its box (`EntityType.Builder.sized`: width, height) and its eyes' height (`eyeHeight`, else
/// 0.85 of the height).
const TYPES: [(&str, &str, f32, f32, f32); 130] = [
    ("minecraft:allay", "net/minecraft/world/entity/animal/allay/Allay", 0.35, 0.6, 0.36),
    ("minecraft:area_effect_cloud", "net/minecraft/world/entity/AreaEffectCloud", 6.0, 0.5, 0.425),
    ("minecraft:armadillo", "net/minecraft/world/entity/animal/armadillo/Armadillo", 0.7, 0.65, 0.26),
    ("minecraft:armor_stand", "net/minecraft/world/entity/decoration/ArmorStand", 0.5, 1.975, 1.7775),
    ("minecraft:arrow", "net/minecraft/world/entity/projectile/Arrow", 0.5, 0.5, 0.13),
    ("minecraft:axolotl", "net/minecraft/world/entity/animal/axolotl/Axolotl", 0.75, 0.42, 0.2751),
    ("minecraft:bat", "net/minecraft/world/entity/ambient/Bat", 0.5, 0.9, 0.45),
    ("minecraft:bee", "net/minecraft/world/entity/animal/Bee", 0.7, 0.6, 0.3),
    ("minecraft:blaze", "net/minecraft/world/entity/monster/Blaze", 0.6, 1.8, 1.53),
    ("minecraft:block_display", "net/minecraft/world/entity/Display$BlockDisplay", 0.0, 0.0, 0.0),
    ("minecraft:boat", "net/minecraft/world/entity/vehicle/Boat", 1.375, 0.5625, 0.5625),
    ("minecraft:bogged", "net/minecraft/world/entity/monster/Bogged", 0.6, 1.99, 1.74),
    ("minecraft:breeze", "net/minecraft/world/entity/monster/breeze/Breeze", 0.6, 1.77, 1.3452),
    ("minecraft:breeze_wind_charge", "net/minecraft/world/entity/projectile/windcharge/BreezeWindCharge", 0.3125, 0.3125, 0.0),
    ("minecraft:camel", "net/minecraft/world/entity/animal/camel/Camel", 1.7, 2.375, 2.275),
    ("minecraft:cat", "net/minecraft/world/entity/animal/Cat", 0.6, 0.7, 0.35),
    ("minecraft:cave_spider", "net/minecraft/world/entity/monster/CaveSpider", 0.7, 0.5, 0.45),
    ("minecraft:chest_boat", "net/minecraft/world/entity/vehicle/ChestBoat", 1.375, 0.5625, 0.5625),
    ("minecraft:chest_minecart", "net/minecraft/world/entity/vehicle/MinecartChest", 0.98, 0.7, 0.595),
    ("minecraft:chicken", "net/minecraft/world/entity/animal/Chicken", 0.4, 0.7, 0.644),
    ("minecraft:cod", "net/minecraft/world/entity/animal/Cod", 0.5, 0.3, 0.195),
    ("minecraft:command_block_minecart", "net/minecraft/world/entity/vehicle/MinecartCommandBlock", 0.98, 0.7, 0.595),
    ("minecraft:cow", "net/minecraft/world/entity/animal/Cow", 0.9, 1.4, 1.3),
    ("minecraft:creeper", "net/minecraft/world/entity/monster/Creeper", 0.6, 1.7, 1.445),
    ("minecraft:dolphin", "net/minecraft/world/entity/animal/Dolphin", 0.9, 0.6, 0.3),
    ("minecraft:donkey", "net/minecraft/world/entity/animal/horse/Donkey", 1.39648, 1.5, 1.425),
    ("minecraft:dragon_fireball", "net/minecraft/world/entity/projectile/DragonFireball", 1.0, 1.0, 0.85),
    ("minecraft:drowned", "net/minecraft/world/entity/monster/Drowned", 0.6, 1.95, 1.74),
    ("minecraft:egg", "net/minecraft/world/entity/projectile/ThrownEgg", 0.25, 0.25, 0.2125),
    ("minecraft:elder_guardian", "net/minecraft/world/entity/monster/ElderGuardian", 1.9975, 1.9975, 0.99875),
    ("minecraft:end_crystal", "net/minecraft/world/entity/boss/enderdragon/EndCrystal", 2.0, 2.0, 1.7),
    ("minecraft:ender_dragon", "net/minecraft/world/entity/boss/enderdragon/EnderDragon", 16.0, 8.0, 6.8),
    ("minecraft:ender_pearl", "net/minecraft/world/entity/projectile/ThrownEnderpearl", 0.25, 0.25, 0.2125),
    ("minecraft:enderman", "net/minecraft/world/entity/monster/EnderMan", 0.6, 2.9, 2.55),
    ("minecraft:endermite", "net/minecraft/world/entity/monster/Endermite", 0.4, 0.3, 0.13),
    ("minecraft:evoker", "net/minecraft/world/entity/monster/Evoker", 0.6, 1.95, 1.6575),
    ("minecraft:evoker_fangs", "net/minecraft/world/entity/projectile/EvokerFangs", 0.5, 0.8, 0.68),
    ("minecraft:experience_bottle", "net/minecraft/world/entity/projectile/ThrownExperienceBottle", 0.25, 0.25, 0.2125),
    ("minecraft:experience_orb", "net/minecraft/world/entity/ExperienceOrb", 0.5, 0.5, 0.425),
    ("minecraft:eye_of_ender", "net/minecraft/world/entity/projectile/EyeOfEnder", 0.25, 0.25, 0.2125),
    ("minecraft:falling_block", "net/minecraft/world/entity/item/FallingBlockEntity", 0.98, 0.98, 0.833),
    ("minecraft:fireball", "net/minecraft/world/entity/projectile/LargeFireball", 1.0, 1.0, 0.85),
    ("minecraft:firework_rocket", "net/minecraft/world/entity/projectile/FireworkRocketEntity", 0.25, 0.25, 0.2125),
    ("minecraft:fishing_bobber", "net/minecraft/world/entity/projectile/FishingHook", 0.25, 0.25, 0.2125),
    ("minecraft:fox", "net/minecraft/world/entity/animal/Fox", 0.6, 0.7, 0.4),
    ("minecraft:frog", "net/minecraft/world/entity/animal/frog/Frog", 0.5, 0.5, 0.425),
    ("minecraft:furnace_minecart", "net/minecraft/world/entity/vehicle/MinecartFurnace", 0.98, 0.7, 0.595),
    ("minecraft:ghast", "net/minecraft/world/entity/monster/Ghast", 4.0, 4.0, 2.6),
    ("minecraft:giant", "net/minecraft/world/entity/monster/Giant", 3.6, 12.0, 10.44),
    ("minecraft:glow_item_frame", "net/minecraft/world/entity/decoration/GlowItemFrame", 0.5, 0.5, 0.0),
    ("minecraft:glow_squid", "net/minecraft/world/entity/GlowSquid", 0.8, 0.8, 0.4),
    ("minecraft:goat", "net/minecraft/world/entity/animal/goat/Goat", 0.9, 1.3, 1.105),
    ("minecraft:guardian", "net/minecraft/world/entity/monster/Guardian", 0.85, 0.85, 0.425),
    ("minecraft:hoglin", "net/minecraft/world/entity/monster/hoglin/Hoglin", 1.39648, 1.4, 1.19),
    ("minecraft:hopper_minecart", "net/minecraft/world/entity/vehicle/MinecartHopper", 0.98, 0.7, 0.595),
    ("minecraft:horse", "net/minecraft/world/entity/animal/horse/Horse", 1.39648, 1.6, 1.52),
    ("minecraft:husk", "net/minecraft/world/entity/monster/Husk", 0.6, 1.95, 1.74),
    ("minecraft:illusioner", "net/minecraft/world/entity/monster/Illusioner", 0.6, 1.95, 1.6575),
    ("minecraft:interaction", "net/minecraft/world/entity/Interaction", 0.0, 0.0, 0.0),
    ("minecraft:iron_golem", "net/minecraft/world/entity/animal/IronGolem", 1.4, 2.7, 2.295),
    ("minecraft:item", "net/minecraft/world/entity/item/ItemEntity", 0.25, 0.25, 0.2125),
    ("minecraft:item_display", "net/minecraft/world/entity/Display$ItemDisplay", 0.0, 0.0, 0.0),
    ("minecraft:item_frame", "net/minecraft/world/entity/decoration/ItemFrame", 0.5, 0.5, 0.0),
    ("minecraft:leash_knot", "net/minecraft/world/entity/decoration/LeashFenceKnotEntity", 0.375, 0.5, 0.0625),
    ("minecraft:lightning_bolt", "net/minecraft/world/entity/LightningBolt", 0.0, 0.0, 0.0),
    ("minecraft:llama", "net/minecraft/world/entity/animal/horse/Llama", 0.9, 1.87, 1.7765),
    ("minecraft:llama_spit", "net/minecraft/world/entity/projectile/LlamaSpit", 0.25, 0.25, 0.2125),
    ("minecraft:magma_cube", "net/minecraft/world/entity/monster/MagmaCube", 0.52, 0.52, 0.325),
    ("minecraft:marker", "net/minecraft/world/entity/Marker", 0.0, 0.0, 0.0),
    ("minecraft:minecart", "net/minecraft/world/entity/vehicle/Minecart", 0.98, 0.7, 0.595),
    ("minecraft:mooshroom", "net/minecraft/world/entity/animal/MushroomCow", 0.9, 1.4, 1.3),
    ("minecraft:mule", "net/minecraft/world/entity/animal/horse/Mule", 1.39648, 1.6, 1.52),
    ("minecraft:ocelot", "net/minecraft/world/entity/animal/Ocelot", 0.6, 0.7, 0.595),
    ("minecraft:ominous_item_spawner", "net/minecraft/world/entity/OminousItemSpawner", 0.25, 0.25, 0.2125),
    ("minecraft:painting", "net/minecraft/world/entity/decoration/Painting", 0.5, 0.5, 0.425),
    ("minecraft:panda", "net/minecraft/world/entity/animal/Panda", 1.3, 1.25, 1.0625),
    ("minecraft:parrot", "net/minecraft/world/entity/animal/Parrot", 0.5, 0.9, 0.54),
    ("minecraft:phantom", "net/minecraft/world/entity/monster/Phantom", 0.9, 0.5, 0.175),
    ("minecraft:pig", "net/minecraft/world/entity/animal/Pig", 0.9, 0.9, 0.765),
    ("minecraft:piglin", "net/minecraft/world/entity/monster/piglin/Piglin", 0.6, 1.95, 1.79),
    ("minecraft:piglin_brute", "net/minecraft/world/entity/monster/piglin/PiglinBrute", 0.6, 1.95, 1.79),
    ("minecraft:pillager", "net/minecraft/world/entity/monster/Pillager", 0.6, 1.95, 1.6575),
    ("minecraft:player", "net/minecraft/world/entity/player/Player", 0.6, 1.8, 1.62),
    ("minecraft:polar_bear", "net/minecraft/world/entity/animal/PolarBear", 1.4, 1.4, 1.19),
    ("minecraft:potion", "net/minecraft/world/entity/projectile/ThrownPotion", 0.25, 0.25, 0.2125),
    ("minecraft:pufferfish", "net/minecraft/world/entity/animal/Pufferfish", 0.7, 0.7, 0.455),
    ("minecraft:rabbit", "net/minecraft/world/entity/animal/Rabbit", 0.4, 0.5, 0.425),
    ("minecraft:ravager", "net/minecraft/world/entity/monster/Ravager", 1.95, 2.2, 1.87),
    ("minecraft:salmon", "net/minecraft/world/entity/animal/Salmon", 0.7, 0.4, 0.26),
    ("minecraft:sheep", "net/minecraft/world/entity/animal/Sheep", 0.9, 1.3, 1.235),
    ("minecraft:shulker", "net/minecraft/world/entity/monster/Shulker", 1.0, 1.0, 0.5),
    ("minecraft:shulker_bullet", "net/minecraft/world/entity/projectile/ShulkerBullet", 0.3125, 0.3125, 0.265625),
    ("minecraft:silverfish", "net/minecraft/world/entity/monster/Silverfish", 0.4, 0.3, 0.13),
    ("minecraft:skeleton", "net/minecraft/world/entity/monster/Skeleton", 0.6, 1.99, 1.74),
    ("minecraft:skeleton_horse", "net/minecraft/world/entity/animal/horse/SkeletonHorse", 1.39648, 1.6, 1.52),
    ("minecraft:slime", "net/minecraft/world/entity/monster/Slime", 0.52, 0.52, 0.325),
    ("minecraft:small_fireball", "net/minecraft/world/entity/projectile/SmallFireball", 0.3125, 0.3125, 0.265625),
    ("minecraft:sniffer", "net/minecraft/world/entity/animal/sniffer/Sniffer", 1.9, 1.75, 1.05),
    ("minecraft:snow_golem", "net/minecraft/world/entity/animal/SnowGolem", 0.7, 1.9, 1.7),
    ("minecraft:snowball", "net/minecraft/world/entity/projectile/Snowball", 0.25, 0.25, 0.2125),
    ("minecraft:spawner_minecart", "net/minecraft/world/entity/vehicle/MinecartSpawner", 0.98, 0.7, 0.595),
    ("minecraft:spectral_arrow", "net/minecraft/world/entity/projectile/SpectralArrow", 0.5, 0.5, 0.13),
    ("minecraft:spider", "net/minecraft/world/entity/monster/Spider", 1.4, 0.9, 0.65),
    ("minecraft:squid", "net/minecraft/world/entity/animal/Squid", 0.8, 0.8, 0.4),
    ("minecraft:stray", "net/minecraft/world/entity/monster/Stray", 0.6, 1.99, 1.74),
    ("minecraft:strider", "net/minecraft/world/entity/monster/Strider", 0.9, 1.7, 1.445),
    ("minecraft:tadpole", "net/minecraft/world/entity/animal/frog/Tadpole", 0.4, 0.3, 0.195),
    ("minecraft:text_display", "net/minecraft/world/entity/Display$TextDisplay", 0.0, 0.0, 0.0),
    ("minecraft:tnt", "net/minecraft/world/entity/item/PrimedTnt", 0.98, 0.98, 0.15),
    ("minecraft:tnt_minecart", "net/minecraft/world/entity/vehicle/MinecartTNT", 0.98, 0.7, 0.595),
    ("minecraft:trader_llama", "net/minecraft/world/entity/animal/horse/TraderLlama", 0.9, 1.87, 1.7765),
    ("minecraft:trident", "net/minecraft/world/entity/projectile/ThrownTrident", 0.5, 0.5, 0.13),
    ("minecraft:tropical_fish", "net/minecraft/world/entity/animal/TropicalFish", 0.5, 0.4, 0.26),
    ("minecraft:turtle", "net/minecraft/world/entity/animal/Turtle", 1.2, 0.4, 0.34),
    ("minecraft:vex", "net/minecraft/world/entity/monster/Vex", 0.4, 0.8, 0.51875),
    ("minecraft:villager", "net/minecraft/world/entity/npc/Villager", 0.6, 1.95, 1.62),
    ("minecraft:vindicator", "net/minecraft/world/entity/monster/Vindicator", 0.6, 1.95, 1.6575),
    ("minecraft:wandering_trader", "net/minecraft/world/entity/npc/WanderingTrader", 0.6, 1.95, 1.62),
    ("minecraft:warden", "net/minecraft/world/entity/monster/warden/Warden", 0.9, 2.9, 2.465),
    ("minecraft:wind_charge", "net/minecraft/world/entity/projectile/windcharge/WindCharge", 0.3125, 0.3125, 0.0),
    ("minecraft:witch", "net/minecraft/world/entity/monster/Witch", 0.6, 1.95, 1.62),
    ("minecraft:wither", "net/minecraft/world/entity/boss/wither/WitherBoss", 0.9, 3.5, 2.975),
    ("minecraft:wither_skeleton", "net/minecraft/world/entity/monster/WitherSkeleton", 0.7, 2.4, 2.1),
    ("minecraft:wither_skull", "net/minecraft/world/entity/projectile/WitherSkull", 0.3125, 0.3125, 0.265625),
    ("minecraft:wolf", "net/minecraft/world/entity/animal/Wolf", 0.6, 0.85, 0.68),
    ("minecraft:zoglin", "net/minecraft/world/entity/monster/Zoglin", 1.39648, 1.4, 1.19),
    ("minecraft:zombie", "net/minecraft/world/entity/monster/Zombie", 0.6, 1.95, 1.74),
    ("minecraft:zombie_horse", "net/minecraft/world/entity/animal/horse/ZombieHorse", 1.39648, 1.6, 1.52),
    ("minecraft:zombie_villager", "net/minecraft/world/entity/monster/ZombieVillager", 0.6, 1.95, 1.74),
    ("minecraft:zombified_piglin", "net/minecraft/world/entity/monster/ZombifiedPiglin", 0.6, 1.95, 1.79),
];

/// The `MobCategory` constant 1.21.1 builds entity type `id` with (`EntityType.Builder.of(factory,
/// category)`): `MISC` for the types not named here, and for a type 1.21.1 lacks.
pub(crate) fn category(id: &str) -> &'static str {
    match id.strip_prefix("minecraft:").unwrap_or(id) {
        "blaze" | "bogged" | "breeze" | "cave_spider" | "creeper" | "drowned" | "elder_guardian" | "ender_dragon" | "enderman" | "endermite"
        | "evoker" | "ghast" | "giant" | "guardian" | "hoglin" | "husk" | "illusioner" | "magma_cube" | "phantom" | "piglin" | "piglin_brute"
        | "pillager" | "ravager" | "shulker" | "silverfish" | "skeleton" | "slime" | "spider" | "stray" | "vex" | "vindicator" | "warden"
        | "witch" | "wither" | "wither_skeleton" | "zoglin" | "zombie" | "zombie_villager" | "zombified_piglin" => "MONSTER",
        "allay" | "armadillo" | "bee" | "camel" | "cat" | "chicken" | "cow" | "donkey" | "fox" | "frog" | "goat" | "horse" | "llama"
        | "mooshroom" | "mule" | "ocelot" | "panda" | "parrot" | "pig" | "polar_bear" | "rabbit" | "sheep" | "skeleton_horse" | "sniffer"
        | "strider" | "tadpole" | "trader_llama" | "turtle" | "wandering_trader" | "wolf" | "zombie_horse" => "CREATURE",
        "bat" => "AMBIENT",
        "axolotl" => "AXOLOTLS",
        "glow_squid" => "UNDERGROUND_WATER_CREATURE",
        "dolphin" | "squid" => "WATER_CREATURE",
        "cod" | "pufferfish" | "salmon" | "tropical_fish" => "WATER_AMBIENT",
        _ => "MISC",
    }
}

/// Those classes' superclasses and interfaces, up to `Entity`, by name: the superclass (none for
/// an interface), then the interfaces the class itself names.
const CLASSES: [(&str, &str, &[&str]); 218] = [
    ("net/minecraft/commands/CommandSource", "", &[]),
    ("net/minecraft/network/syncher/SyncedDataHolder", "", &[]),
    ("net/minecraft/world/Clearable", "", &[]),
    ("net/minecraft/world/Container", "", &["net/minecraft/world/Clearable"]),
    ("net/minecraft/world/ContainerListener", "", &[]),
    ("net/minecraft/world/MenuProvider", "", &["net/minecraft/world/inventory/MenuConstructor", "net/neoforged/neoforge/client/extensions/IMenuProviderExtension"]),
    ("net/minecraft/world/Nameable", "", &[]),
    ("net/minecraft/world/entity/AgeableMob", "net/minecraft/world/entity/PathfinderMob", &[]),
    ("net/minecraft/world/entity/AreaEffectCloud", "net/minecraft/world/entity/Entity", &["net/minecraft/world/entity/TraceableEntity"]),
    ("net/minecraft/world/entity/Attackable", "", &[]),
    ("net/minecraft/world/entity/Display", "net/minecraft/world/entity/Entity", &[]),
    ("net/minecraft/world/entity/Display$BlockDisplay", "net/minecraft/world/entity/Display", &[]),
    ("net/minecraft/world/entity/Display$ItemDisplay", "net/minecraft/world/entity/Display", &[]),
    ("net/minecraft/world/entity/Display$TextDisplay", "net/minecraft/world/entity/Display", &[]),
    ("net/minecraft/world/entity/Entity", "net/neoforged/neoforge/attachment/AttachmentHolder", &["net/minecraft/network/syncher/SyncedDataHolder", "net/minecraft/world/Nameable", "net/minecraft/world/level/entity/EntityAccess", "net/minecraft/commands/CommandSource", "net/minecraft/world/scores/ScoreHolder", "net/neoforged/neoforge/common/extensions/IEntityExtension"]),
    ("net/minecraft/world/entity/EquipmentUser", "", &[]),
    ("net/minecraft/world/entity/ExperienceOrb", "net/minecraft/world/entity/Entity", &[]),
    ("net/minecraft/world/entity/FlyingMob", "net/minecraft/world/entity/Mob", &[]),
    ("net/minecraft/world/entity/GlowSquid", "net/minecraft/world/entity/animal/Squid", &[]),
    ("net/minecraft/world/entity/HasCustomInventoryScreen", "", &[]),
    ("net/minecraft/world/entity/Interaction", "net/minecraft/world/entity/Entity", &["net/minecraft/world/entity/Attackable", "net/minecraft/world/entity/Targeting"]),
    ("net/minecraft/world/entity/ItemSteerable", "", &[]),
    ("net/minecraft/world/entity/Leashable", "", &[]),
    ("net/minecraft/world/entity/LerpingModel", "", &[]),
    ("net/minecraft/world/entity/LightningBolt", "net/minecraft/world/entity/Entity", &[]),
    ("net/minecraft/world/entity/LivingEntity", "net/minecraft/world/entity/Entity", &["net/minecraft/world/entity/Attackable", "net/neoforged/neoforge/common/extensions/ILivingEntityExtension"]),
    ("net/minecraft/world/entity/Marker", "net/minecraft/world/entity/Entity", &[]),
    ("net/minecraft/world/entity/Mob", "net/minecraft/world/entity/LivingEntity", &["net/minecraft/world/entity/EquipmentUser", "net/minecraft/world/entity/Leashable", "net/minecraft/world/entity/Targeting"]),
    ("net/minecraft/world/entity/NeutralMob", "", &[]),
    ("net/minecraft/world/entity/OminousItemSpawner", "net/minecraft/world/entity/Entity", &[]),
    ("net/minecraft/world/entity/OwnableEntity", "", &[]),
    ("net/minecraft/world/entity/PathfinderMob", "net/minecraft/world/entity/Mob", &[]),
    ("net/minecraft/world/entity/PlayerRideable", "", &[]),
    ("net/minecraft/world/entity/PlayerRideableJumping", "", &["net/minecraft/world/entity/PlayerRideable"]),
    ("net/minecraft/world/entity/PowerableMob", "", &[]),
    ("net/minecraft/world/entity/ReputationEventHandler", "", &[]),
    ("net/minecraft/world/entity/Saddleable", "", &[]),
    ("net/minecraft/world/entity/Shearable", "", &["net/neoforged/neoforge/common/IShearable"]),
    ("net/minecraft/world/entity/TamableAnimal", "net/minecraft/world/entity/animal/Animal", &["net/minecraft/world/entity/OwnableEntity"]),
    ("net/minecraft/world/entity/Targeting", "", &[]),
    ("net/minecraft/world/entity/TraceableEntity", "", &[]),
    ("net/minecraft/world/entity/VariantHolder", "", &[]),
    ("net/minecraft/world/entity/ambient/AmbientCreature", "net/minecraft/world/entity/Mob", &[]),
    ("net/minecraft/world/entity/ambient/Bat", "net/minecraft/world/entity/ambient/AmbientCreature", &[]),
    ("net/minecraft/world/entity/animal/AbstractFish", "net/minecraft/world/entity/animal/WaterAnimal", &["net/minecraft/world/entity/animal/Bucketable"]),
    ("net/minecraft/world/entity/animal/AbstractGolem", "net/minecraft/world/entity/PathfinderMob", &[]),
    ("net/minecraft/world/entity/animal/AbstractSchoolingFish", "net/minecraft/world/entity/animal/AbstractFish", &[]),
    ("net/minecraft/world/entity/animal/Animal", "net/minecraft/world/entity/AgeableMob", &[]),
    ("net/minecraft/world/entity/animal/Bee", "net/minecraft/world/entity/animal/Animal", &["net/minecraft/world/entity/NeutralMob", "net/minecraft/world/entity/animal/FlyingAnimal"]),
    ("net/minecraft/world/entity/animal/Bucketable", "", &[]),
    ("net/minecraft/world/entity/animal/Cat", "net/minecraft/world/entity/TamableAnimal", &["net/minecraft/world/entity/VariantHolder"]),
    ("net/minecraft/world/entity/animal/Chicken", "net/minecraft/world/entity/animal/Animal", &[]),
    ("net/minecraft/world/entity/animal/Cod", "net/minecraft/world/entity/animal/AbstractSchoolingFish", &[]),
    ("net/minecraft/world/entity/animal/Cow", "net/minecraft/world/entity/animal/Animal", &[]),
    ("net/minecraft/world/entity/animal/Dolphin", "net/minecraft/world/entity/animal/WaterAnimal", &[]),
    ("net/minecraft/world/entity/animal/FlyingAnimal", "", &[]),
    ("net/minecraft/world/entity/animal/Fox", "net/minecraft/world/entity/animal/Animal", &["net/minecraft/world/entity/VariantHolder"]),
    ("net/minecraft/world/entity/animal/IronGolem", "net/minecraft/world/entity/animal/AbstractGolem", &["net/minecraft/world/entity/NeutralMob"]),
    ("net/minecraft/world/entity/animal/MushroomCow", "net/minecraft/world/entity/animal/Cow", &["net/minecraft/world/entity/Shearable", "net/minecraft/world/entity/VariantHolder"]),
    ("net/minecraft/world/entity/animal/Ocelot", "net/minecraft/world/entity/animal/Animal", &[]),
    ("net/minecraft/world/entity/animal/Panda", "net/minecraft/world/entity/animal/Animal", &[]),
    ("net/minecraft/world/entity/animal/Parrot", "net/minecraft/world/entity/animal/ShoulderRidingEntity", &["net/minecraft/world/entity/VariantHolder", "net/minecraft/world/entity/animal/FlyingAnimal"]),
    ("net/minecraft/world/entity/animal/Pig", "net/minecraft/world/entity/animal/Animal", &["net/minecraft/world/entity/ItemSteerable", "net/minecraft/world/entity/Saddleable"]),
    ("net/minecraft/world/entity/animal/PolarBear", "net/minecraft/world/entity/animal/Animal", &["net/minecraft/world/entity/NeutralMob"]),
    ("net/minecraft/world/entity/animal/Pufferfish", "net/minecraft/world/entity/animal/AbstractFish", &[]),
    ("net/minecraft/world/entity/animal/Rabbit", "net/minecraft/world/entity/animal/Animal", &["net/minecraft/world/entity/VariantHolder"]),
    ("net/minecraft/world/entity/animal/Salmon", "net/minecraft/world/entity/animal/AbstractSchoolingFish", &[]),
    ("net/minecraft/world/entity/animal/Sheep", "net/minecraft/world/entity/animal/Animal", &["net/minecraft/world/entity/Shearable"]),
    ("net/minecraft/world/entity/animal/ShoulderRidingEntity", "net/minecraft/world/entity/TamableAnimal", &[]),
    ("net/minecraft/world/entity/animal/SnowGolem", "net/minecraft/world/entity/animal/AbstractGolem", &["net/minecraft/world/entity/Shearable", "net/minecraft/world/entity/monster/RangedAttackMob"]),
    ("net/minecraft/world/entity/animal/Squid", "net/minecraft/world/entity/animal/WaterAnimal", &[]),
    ("net/minecraft/world/entity/animal/TropicalFish", "net/minecraft/world/entity/animal/AbstractSchoolingFish", &["net/minecraft/world/entity/VariantHolder"]),
    ("net/minecraft/world/entity/animal/Turtle", "net/minecraft/world/entity/animal/Animal", &[]),
    ("net/minecraft/world/entity/animal/WaterAnimal", "net/minecraft/world/entity/PathfinderMob", &[]),
    ("net/minecraft/world/entity/animal/Wolf", "net/minecraft/world/entity/TamableAnimal", &["net/minecraft/world/entity/NeutralMob", "net/minecraft/world/entity/VariantHolder"]),
    ("net/minecraft/world/entity/animal/allay/Allay", "net/minecraft/world/entity/PathfinderMob", &["net/minecraft/world/entity/npc/InventoryCarrier", "net/minecraft/world/level/gameevent/vibrations/VibrationSystem"]),
    ("net/minecraft/world/entity/animal/armadillo/Armadillo", "net/minecraft/world/entity/animal/Animal", &[]),
    ("net/minecraft/world/entity/animal/axolotl/Axolotl", "net/minecraft/world/entity/animal/Animal", &["net/minecraft/world/entity/LerpingModel", "net/minecraft/world/entity/VariantHolder", "net/minecraft/world/entity/animal/Bucketable"]),
    ("net/minecraft/world/entity/animal/camel/Camel", "net/minecraft/world/entity/animal/horse/AbstractHorse", &["net/minecraft/world/entity/PlayerRideableJumping", "net/minecraft/world/entity/Saddleable"]),
    ("net/minecraft/world/entity/animal/frog/Frog", "net/minecraft/world/entity/animal/Animal", &["net/minecraft/world/entity/VariantHolder"]),
    ("net/minecraft/world/entity/animal/frog/Tadpole", "net/minecraft/world/entity/animal/AbstractFish", &[]),
    ("net/minecraft/world/entity/animal/goat/Goat", "net/minecraft/world/entity/animal/Animal", &[]),
    ("net/minecraft/world/entity/animal/horse/AbstractChestedHorse", "net/minecraft/world/entity/animal/horse/AbstractHorse", &[]),
    ("net/minecraft/world/entity/animal/horse/AbstractHorse", "net/minecraft/world/entity/animal/Animal", &["net/minecraft/world/ContainerListener", "net/minecraft/world/entity/HasCustomInventoryScreen", "net/minecraft/world/entity/OwnableEntity", "net/minecraft/world/entity/PlayerRideableJumping", "net/minecraft/world/entity/Saddleable"]),
    ("net/minecraft/world/entity/animal/horse/Donkey", "net/minecraft/world/entity/animal/horse/AbstractChestedHorse", &[]),
    ("net/minecraft/world/entity/animal/horse/Horse", "net/minecraft/world/entity/animal/horse/AbstractHorse", &["net/minecraft/world/entity/VariantHolder"]),
    ("net/minecraft/world/entity/animal/horse/Llama", "net/minecraft/world/entity/animal/horse/AbstractChestedHorse", &["net/minecraft/world/entity/VariantHolder", "net/minecraft/world/entity/monster/RangedAttackMob"]),
    ("net/minecraft/world/entity/animal/horse/Mule", "net/minecraft/world/entity/animal/horse/AbstractChestedHorse", &[]),
    ("net/minecraft/world/entity/animal/horse/SkeletonHorse", "net/minecraft/world/entity/animal/horse/AbstractHorse", &[]),
    ("net/minecraft/world/entity/animal/horse/TraderLlama", "net/minecraft/world/entity/animal/horse/Llama", &[]),
    ("net/minecraft/world/entity/animal/horse/ZombieHorse", "net/minecraft/world/entity/animal/horse/AbstractHorse", &[]),
    ("net/minecraft/world/entity/animal/sniffer/Sniffer", "net/minecraft/world/entity/animal/Animal", &[]),
    ("net/minecraft/world/entity/boss/enderdragon/EndCrystal", "net/minecraft/world/entity/Entity", &[]),
    ("net/minecraft/world/entity/boss/enderdragon/EnderDragon", "net/minecraft/world/entity/Mob", &["net/minecraft/world/entity/monster/Enemy"]),
    ("net/minecraft/world/entity/boss/wither/WitherBoss", "net/minecraft/world/entity/monster/Monster", &["net/minecraft/world/entity/PowerableMob", "net/minecraft/world/entity/monster/RangedAttackMob"]),
    ("net/minecraft/world/entity/decoration/ArmorStand", "net/minecraft/world/entity/LivingEntity", &[]),
    ("net/minecraft/world/entity/decoration/BlockAttachedEntity", "net/minecraft/world/entity/Entity", &[]),
    ("net/minecraft/world/entity/decoration/GlowItemFrame", "net/minecraft/world/entity/decoration/ItemFrame", &[]),
    ("net/minecraft/world/entity/decoration/HangingEntity", "net/minecraft/world/entity/decoration/BlockAttachedEntity", &[]),
    ("net/minecraft/world/entity/decoration/ItemFrame", "net/minecraft/world/entity/decoration/HangingEntity", &[]),
    ("net/minecraft/world/entity/decoration/LeashFenceKnotEntity", "net/minecraft/world/entity/decoration/BlockAttachedEntity", &[]),
    ("net/minecraft/world/entity/decoration/Painting", "net/minecraft/world/entity/decoration/HangingEntity", &["net/minecraft/world/entity/VariantHolder"]),
    ("net/minecraft/world/entity/item/FallingBlockEntity", "net/minecraft/world/entity/Entity", &[]),
    ("net/minecraft/world/entity/item/ItemEntity", "net/minecraft/world/entity/Entity", &["net/minecraft/world/entity/TraceableEntity"]),
    ("net/minecraft/world/entity/item/PrimedTnt", "net/minecraft/world/entity/Entity", &["net/minecraft/world/entity/TraceableEntity"]),
    ("net/minecraft/world/entity/monster/AbstractIllager", "net/minecraft/world/entity/raid/Raider", &[]),
    ("net/minecraft/world/entity/monster/AbstractSkeleton", "net/minecraft/world/entity/monster/Monster", &["net/minecraft/world/entity/monster/RangedAttackMob"]),
    ("net/minecraft/world/entity/monster/Blaze", "net/minecraft/world/entity/monster/Monster", &[]),
    ("net/minecraft/world/entity/monster/Bogged", "net/minecraft/world/entity/monster/AbstractSkeleton", &["net/minecraft/world/entity/Shearable"]),
    ("net/minecraft/world/entity/monster/CaveSpider", "net/minecraft/world/entity/monster/Spider", &[]),
    ("net/minecraft/world/entity/monster/Creeper", "net/minecraft/world/entity/monster/Monster", &["net/minecraft/world/entity/PowerableMob"]),
    ("net/minecraft/world/entity/monster/CrossbowAttackMob", "", &["net/minecraft/world/entity/monster/RangedAttackMob"]),
    ("net/minecraft/world/entity/monster/Drowned", "net/minecraft/world/entity/monster/Zombie", &["net/minecraft/world/entity/monster/RangedAttackMob"]),
    ("net/minecraft/world/entity/monster/ElderGuardian", "net/minecraft/world/entity/monster/Guardian", &[]),
    ("net/minecraft/world/entity/monster/EnderMan", "net/minecraft/world/entity/monster/Monster", &["net/minecraft/world/entity/NeutralMob"]),
    ("net/minecraft/world/entity/monster/Endermite", "net/minecraft/world/entity/monster/Monster", &[]),
    ("net/minecraft/world/entity/monster/Enemy", "", &[]),
    ("net/minecraft/world/entity/monster/Evoker", "net/minecraft/world/entity/monster/SpellcasterIllager", &[]),
    ("net/minecraft/world/entity/monster/Ghast", "net/minecraft/world/entity/FlyingMob", &["net/minecraft/world/entity/monster/Enemy"]),
    ("net/minecraft/world/entity/monster/Giant", "net/minecraft/world/entity/monster/Monster", &[]),
    ("net/minecraft/world/entity/monster/Guardian", "net/minecraft/world/entity/monster/Monster", &[]),
    ("net/minecraft/world/entity/monster/Husk", "net/minecraft/world/entity/monster/Zombie", &[]),
    ("net/minecraft/world/entity/monster/Illusioner", "net/minecraft/world/entity/monster/SpellcasterIllager", &["net/minecraft/world/entity/monster/RangedAttackMob"]),
    ("net/minecraft/world/entity/monster/MagmaCube", "net/minecraft/world/entity/monster/Slime", &[]),
    ("net/minecraft/world/entity/monster/Monster", "net/minecraft/world/entity/PathfinderMob", &["net/minecraft/world/entity/monster/Enemy"]),
    ("net/minecraft/world/entity/monster/PatrollingMonster", "net/minecraft/world/entity/monster/Monster", &[]),
    ("net/minecraft/world/entity/monster/Phantom", "net/minecraft/world/entity/FlyingMob", &["net/minecraft/world/entity/monster/Enemy"]),
    ("net/minecraft/world/entity/monster/Pillager", "net/minecraft/world/entity/monster/AbstractIllager", &["net/minecraft/world/entity/monster/CrossbowAttackMob", "net/minecraft/world/entity/npc/InventoryCarrier"]),
    ("net/minecraft/world/entity/monster/RangedAttackMob", "", &[]),
    ("net/minecraft/world/entity/monster/Ravager", "net/minecraft/world/entity/raid/Raider", &[]),
    ("net/minecraft/world/entity/monster/Shulker", "net/minecraft/world/entity/animal/AbstractGolem", &["net/minecraft/world/entity/VariantHolder", "net/minecraft/world/entity/monster/Enemy"]),
    ("net/minecraft/world/entity/monster/Silverfish", "net/minecraft/world/entity/monster/Monster", &[]),
    ("net/minecraft/world/entity/monster/Skeleton", "net/minecraft/world/entity/monster/AbstractSkeleton", &[]),
    ("net/minecraft/world/entity/monster/Slime", "net/minecraft/world/entity/Mob", &["net/minecraft/world/entity/monster/Enemy"]),
    ("net/minecraft/world/entity/monster/SpellcasterIllager", "net/minecraft/world/entity/monster/AbstractIllager", &[]),
    ("net/minecraft/world/entity/monster/Spider", "net/minecraft/world/entity/monster/Monster", &[]),
    ("net/minecraft/world/entity/monster/Stray", "net/minecraft/world/entity/monster/AbstractSkeleton", &[]),
    ("net/minecraft/world/entity/monster/Strider", "net/minecraft/world/entity/animal/Animal", &["net/minecraft/world/entity/ItemSteerable", "net/minecraft/world/entity/Saddleable"]),
    ("net/minecraft/world/entity/monster/Vex", "net/minecraft/world/entity/monster/Monster", &["net/minecraft/world/entity/TraceableEntity"]),
    ("net/minecraft/world/entity/monster/Vindicator", "net/minecraft/world/entity/monster/AbstractIllager", &[]),
    ("net/minecraft/world/entity/monster/Witch", "net/minecraft/world/entity/raid/Raider", &["net/minecraft/world/entity/monster/RangedAttackMob"]),
    ("net/minecraft/world/entity/monster/WitherSkeleton", "net/minecraft/world/entity/monster/AbstractSkeleton", &[]),
    ("net/minecraft/world/entity/monster/Zoglin", "net/minecraft/world/entity/monster/Monster", &["net/minecraft/world/entity/monster/Enemy", "net/minecraft/world/entity/monster/hoglin/HoglinBase"]),
    ("net/minecraft/world/entity/monster/Zombie", "net/minecraft/world/entity/monster/Monster", &[]),
    ("net/minecraft/world/entity/monster/ZombieVillager", "net/minecraft/world/entity/monster/Zombie", &["net/minecraft/world/entity/npc/VillagerDataHolder"]),
    ("net/minecraft/world/entity/monster/ZombifiedPiglin", "net/minecraft/world/entity/monster/Zombie", &["net/minecraft/world/entity/NeutralMob"]),
    ("net/minecraft/world/entity/monster/breeze/Breeze", "net/minecraft/world/entity/monster/Monster", &[]),
    ("net/minecraft/world/entity/monster/hoglin/Hoglin", "net/minecraft/world/entity/animal/Animal", &["net/minecraft/world/entity/monster/Enemy", "net/minecraft/world/entity/monster/hoglin/HoglinBase"]),
    ("net/minecraft/world/entity/monster/hoglin/HoglinBase", "", &[]),
    ("net/minecraft/world/entity/monster/piglin/AbstractPiglin", "net/minecraft/world/entity/monster/Monster", &[]),
    ("net/minecraft/world/entity/monster/piglin/Piglin", "net/minecraft/world/entity/monster/piglin/AbstractPiglin", &["net/minecraft/world/entity/monster/CrossbowAttackMob", "net/minecraft/world/entity/npc/InventoryCarrier"]),
    ("net/minecraft/world/entity/monster/piglin/PiglinBrute", "net/minecraft/world/entity/monster/piglin/AbstractPiglin", &[]),
    ("net/minecraft/world/entity/monster/warden/Warden", "net/minecraft/world/entity/monster/Monster", &["net/minecraft/world/level/gameevent/vibrations/VibrationSystem"]),
    ("net/minecraft/world/entity/npc/AbstractVillager", "net/minecraft/world/entity/AgeableMob", &["net/minecraft/world/entity/npc/InventoryCarrier", "net/minecraft/world/entity/npc/Npc", "net/minecraft/world/item/trading/Merchant"]),
    ("net/minecraft/world/entity/npc/InventoryCarrier", "", &[]),
    ("net/minecraft/world/entity/npc/Npc", "", &[]),
    ("net/minecraft/world/entity/npc/Villager", "net/minecraft/world/entity/npc/AbstractVillager", &["net/minecraft/world/entity/ReputationEventHandler", "net/minecraft/world/entity/npc/VillagerDataHolder"]),
    ("net/minecraft/world/entity/npc/VillagerDataHolder", "", &["net/minecraft/world/entity/VariantHolder"]),
    ("net/minecraft/world/entity/npc/WanderingTrader", "net/minecraft/world/entity/npc/AbstractVillager", &[]),
    ("net/minecraft/world/entity/player/Player", "net/minecraft/world/entity/LivingEntity", &["net/neoforged/neoforge/common/extensions/IPlayerExtension"]),
    ("net/minecraft/world/entity/projectile/AbstractArrow", "net/minecraft/world/entity/projectile/Projectile", &[]),
    ("net/minecraft/world/entity/projectile/AbstractHurtingProjectile", "net/minecraft/world/entity/projectile/Projectile", &[]),
    ("net/minecraft/world/entity/projectile/Arrow", "net/minecraft/world/entity/projectile/AbstractArrow", &[]),
    ("net/minecraft/world/entity/projectile/DragonFireball", "net/minecraft/world/entity/projectile/AbstractHurtingProjectile", &[]),
    ("net/minecraft/world/entity/projectile/EvokerFangs", "net/minecraft/world/entity/Entity", &["net/minecraft/world/entity/TraceableEntity"]),
    ("net/minecraft/world/entity/projectile/EyeOfEnder", "net/minecraft/world/entity/Entity", &["net/minecraft/world/entity/projectile/ItemSupplier"]),
    ("net/minecraft/world/entity/projectile/Fireball", "net/minecraft/world/entity/projectile/AbstractHurtingProjectile", &["net/minecraft/world/entity/projectile/ItemSupplier"]),
    ("net/minecraft/world/entity/projectile/FireworkRocketEntity", "net/minecraft/world/entity/projectile/Projectile", &["net/minecraft/world/entity/projectile/ItemSupplier"]),
    ("net/minecraft/world/entity/projectile/FishingHook", "net/minecraft/world/entity/projectile/Projectile", &[]),
    ("net/minecraft/world/entity/projectile/ItemSupplier", "", &[]),
    ("net/minecraft/world/entity/projectile/LargeFireball", "net/minecraft/world/entity/projectile/Fireball", &[]),
    ("net/minecraft/world/entity/projectile/LlamaSpit", "net/minecraft/world/entity/projectile/Projectile", &[]),
    ("net/minecraft/world/entity/projectile/Projectile", "net/minecraft/world/entity/Entity", &["net/minecraft/world/entity/TraceableEntity"]),
    ("net/minecraft/world/entity/projectile/ShulkerBullet", "net/minecraft/world/entity/projectile/Projectile", &[]),
    ("net/minecraft/world/entity/projectile/SmallFireball", "net/minecraft/world/entity/projectile/Fireball", &[]),
    ("net/minecraft/world/entity/projectile/Snowball", "net/minecraft/world/entity/projectile/ThrowableItemProjectile", &[]),
    ("net/minecraft/world/entity/projectile/SpectralArrow", "net/minecraft/world/entity/projectile/AbstractArrow", &[]),
    ("net/minecraft/world/entity/projectile/ThrowableItemProjectile", "net/minecraft/world/entity/projectile/ThrowableProjectile", &["net/minecraft/world/entity/projectile/ItemSupplier"]),
    ("net/minecraft/world/entity/projectile/ThrowableProjectile", "net/minecraft/world/entity/projectile/Projectile", &[]),
    ("net/minecraft/world/entity/projectile/ThrownEgg", "net/minecraft/world/entity/projectile/ThrowableItemProjectile", &[]),
    ("net/minecraft/world/entity/projectile/ThrownEnderpearl", "net/minecraft/world/entity/projectile/ThrowableItemProjectile", &[]),
    ("net/minecraft/world/entity/projectile/ThrownExperienceBottle", "net/minecraft/world/entity/projectile/ThrowableItemProjectile", &[]),
    ("net/minecraft/world/entity/projectile/ThrownPotion", "net/minecraft/world/entity/projectile/ThrowableItemProjectile", &["net/minecraft/world/entity/projectile/ItemSupplier"]),
    ("net/minecraft/world/entity/projectile/ThrownTrident", "net/minecraft/world/entity/projectile/AbstractArrow", &[]),
    ("net/minecraft/world/entity/projectile/WitherSkull", "net/minecraft/world/entity/projectile/AbstractHurtingProjectile", &[]),
    ("net/minecraft/world/entity/projectile/windcharge/AbstractWindCharge", "net/minecraft/world/entity/projectile/AbstractHurtingProjectile", &["net/minecraft/world/entity/projectile/ItemSupplier"]),
    ("net/minecraft/world/entity/projectile/windcharge/BreezeWindCharge", "net/minecraft/world/entity/projectile/windcharge/AbstractWindCharge", &[]),
    ("net/minecraft/world/entity/projectile/windcharge/WindCharge", "net/minecraft/world/entity/projectile/windcharge/AbstractWindCharge", &[]),
    ("net/minecraft/world/entity/raid/Raider", "net/minecraft/world/entity/monster/PatrollingMonster", &[]),
    ("net/minecraft/world/entity/vehicle/AbstractMinecart", "net/minecraft/world/entity/vehicle/VehicleEntity", &["net/neoforged/neoforge/common/extensions/IAbstractMinecartExtension"]),
    ("net/minecraft/world/entity/vehicle/AbstractMinecartContainer", "net/minecraft/world/entity/vehicle/AbstractMinecart", &["net/minecraft/world/entity/vehicle/ContainerEntity"]),
    ("net/minecraft/world/entity/vehicle/Boat", "net/minecraft/world/entity/vehicle/VehicleEntity", &["net/minecraft/world/entity/Leashable", "net/minecraft/world/entity/VariantHolder", "net/neoforged/neoforge/common/extensions/IBoatExtension"]),
    ("net/minecraft/world/entity/vehicle/ChestBoat", "net/minecraft/world/entity/vehicle/Boat", &["net/minecraft/world/entity/HasCustomInventoryScreen", "net/minecraft/world/entity/vehicle/ContainerEntity"]),
    ("net/minecraft/world/entity/vehicle/ContainerEntity", "", &["net/minecraft/world/Container", "net/minecraft/world/MenuProvider"]),
    ("net/minecraft/world/entity/vehicle/Minecart", "net/minecraft/world/entity/vehicle/AbstractMinecart", &[]),
    ("net/minecraft/world/entity/vehicle/MinecartChest", "net/minecraft/world/entity/vehicle/AbstractMinecartContainer", &[]),
    ("net/minecraft/world/entity/vehicle/MinecartCommandBlock", "net/minecraft/world/entity/vehicle/AbstractMinecart", &[]),
    ("net/minecraft/world/entity/vehicle/MinecartFurnace", "net/minecraft/world/entity/vehicle/AbstractMinecart", &[]),
    ("net/minecraft/world/entity/vehicle/MinecartHopper", "net/minecraft/world/entity/vehicle/AbstractMinecartContainer", &["net/minecraft/world/level/block/entity/Hopper"]),
    ("net/minecraft/world/entity/vehicle/MinecartSpawner", "net/minecraft/world/entity/vehicle/AbstractMinecart", &[]),
    ("net/minecraft/world/entity/vehicle/MinecartTNT", "net/minecraft/world/entity/vehicle/AbstractMinecart", &[]),
    ("net/minecraft/world/entity/vehicle/VehicleEntity", "net/minecraft/world/entity/Entity", &[]),
    ("net/minecraft/world/inventory/MenuConstructor", "", &[]),
    ("net/minecraft/world/item/trading/Merchant", "", &[]),
    ("net/minecraft/world/level/block/entity/Hopper", "", &["net/minecraft/world/Container"]),
    ("net/minecraft/world/level/entity/EntityAccess", "", &[]),
    ("net/minecraft/world/level/gameevent/vibrations/VibrationSystem", "", &[]),
    ("net/minecraft/world/scores/ScoreHolder", "", &[]),
    ("net/neoforged/neoforge/attachment/AttachmentHolder", "", &["net/neoforged/neoforge/attachment/IAttachmentHolder"]),
    ("net/neoforged/neoforge/attachment/IAttachmentHolder", "", &[]),
    ("net/neoforged/neoforge/client/extensions/IMenuProviderExtension", "", &[]),
    ("net/neoforged/neoforge/common/IShearable", "", &[]),
    ("net/neoforged/neoforge/common/extensions/IAbstractMinecartExtension", "", &[]),
    ("net/neoforged/neoforge/common/extensions/IBoatExtension", "", &[]),
    ("net/neoforged/neoforge/common/extensions/IEntityExtension", "", &["net/neoforged/neoforge/common/util/INBTSerializable"]),
    ("net/neoforged/neoforge/common/extensions/ILivingEntityExtension", "", &["net/neoforged/neoforge/common/extensions/IEntityExtension"]),
    ("net/neoforged/neoforge/common/extensions/IPlayerExtension", "", &[]),
    ("net/neoforged/neoforge/common/util/INBTSerializable", "", &[]),
];

fn type_entry(kind: &str) -> Option<&'static (&'static str, &'static str, f32, f32, f32)> {
    TYPES.binary_search_by(|t| t.0.cmp(kind)).ok().map(|i| &TYPES[i])
}

/// Whether `id` names one of 1.21.1's entity types (`minecraft:cow`).
pub(crate) fn is_vanilla_type(id: &str) -> bool {
    type_entry(id).is_some()
}

/// The superclass of 1.21.1's entity class `class` (`LivingEntity`'s `Entity`): a call a mod's
/// entity makes through its vanilla class climbs to the class that answers it.
pub(crate) fn api_super(class: &str) -> Option<&'static str> {
    class_entry(class).map(|e| e.1).filter(|s| !s.is_empty())
}

fn class_entry(c: &str) -> Option<&'static (&'static str, &'static str, &'static [&'static str])> {
    CLASSES.binary_search_by(|e| e.0.cmp(c)).ok().map(|i| &CLASSES[i])
}

/// The 1.21.1 class an entity of type `kind` is made of; `living` when the game gives it health.
pub fn class_for(kind: &str, living: bool) -> &'static str {
    if let Some(t) = type_entry(kind) {
        return t.1;
    }
    let path = kind.strip_prefix("minecraft:").unwrap_or(kind);
    if path.ends_with("_chest_boat") || path.ends_with("_chest_raft") {
        CHEST_BOAT
    } else if path.ends_with("_boat") || path.ends_with("_raft") {
        BOAT
    } else if living {
        PATHFINDER
    } else {
        ENTITY
    }
}

/// Vanilla's entity type `kind`'s box (width, height) and eye height, as [`size`] gives them.
pub(crate) fn dimensions(kind: &str) -> (f32, f32, f32) {
    size(kind, false)
}

pub(crate) const POSE_DIMENSIONS: &str =
    "(Lnet/minecraft/world/entity/Pose;)Lnet/minecraft/world/entity/EntityDimensions;";

/// `getDimensions(pose)` (and `getDefaultDimensions`) of vanilla's entity of type `kind`: its
/// type's (`EntityType.getDimensions`); a living one's asleep a fixed fifth of a block
/// (`LivingEntity.SLEEPING_DIMENSIONS`), else its type's at its age's and its own scale, which
/// the game doesn't tell here (a grown one's, 1).
pub(crate) fn pose_dimensions(rt: &mut Rt, kind: &str, living: bool, name: &str, pose: V) -> V {
    let sleeping = pose.r() != 0 && rt.str_of(rt.get_field(pose, "$name")) == Some("SLEEPING");
    if living && name == "getDimensions" && sleeping {
        return crate::mod_entities::sized(rt, 0.2, 0.2, 0.2, true, 0.0);
    }
    let (w, h, eye) = size(kind, living);
    crate::mod_entities::sized(rt, w, h, eye, false, 0.0)
}

/// Whether vanilla's entity type `kind` is of a living class (a mob, an armor stand).
pub(crate) fn is_living_type(kind: &str) -> bool {
    type_entry(kind).is_some_and(|t| is_a(t.1, "net/minecraft/world/entity/LivingEntity"))
}

/// An entity type's box (width, height) and eye height: 1.21.1's, a boat's for 26.3's boats, a
/// player's size for another mob it lacks, half a block for the rest.
fn size(kind: &str, living: bool) -> (f32, f32, f32) {
    if let Some(t) = type_entry(kind) {
        return (t.2, t.3, t.4);
    }
    match class_for(kind, living) {
        BOAT | CHEST_BOAT => (1.375, 0.5625, 0.5625),
        PATHFINDER => (0.6, 1.8, 1.53),
        _ => (0.5, 0.5, 0.425),
    }
}

/// Whether the vanilla entity class `c` is `class` or below it: its superclasses and every
/// interface on the way, as `instanceof` answers.
pub fn is_a(c: &str, class: &str) -> bool {
    if class_entry(c).is_none() {
        return false;
    }
    let mut todo = vec![c];
    while let Some(k) = todo.pop() {
        if k == class {
            return true;
        }
        if let Some((_, sup, ifs)) = class_entry(k) {
            if !sup.is_empty() {
                todo.push(sup);
            }
            todo.extend_from_slice(ifs);
        }
    }
    false
}

fn proxies(rt: &mut Rt) -> V {
    rt.intern(LIST.into(), |rt| crate::jutil::new_list(rt, Vec::new()))
}

fn set_list(rt: &mut Rt, list: V, items: Vec<V>) {
    if let Some(crate::Data::List(l)) = rt.heap.get_mut(list.r() as usize).map(|o| &mut o.data) {
        *l = items;
    }
}

fn game_time(rt: &Rt) -> i64 {
    rt.host().map_or(0, |h| h.game_time())
}

/// Whether `v` is one of these objects.
pub fn is_proxy(rt: &Rt, v: V) -> bool {
    matches!(v, V::R(n) if n != 0) && rt.get_field(v, "$vanilla").i() == 1
}

/// The game's entity id `e` stands for.
pub(crate) fn id(rt: &Rt, e: V) -> i32 {
    rt.get_field(e, "$vid").i()
}

fn pos(rt: &Rt, e: V) -> [f64; 3] {
    [rt.get_field(e, "$x").d(), rt.get_field(e, "$y").d(), rt.get_field(e, "$z").d()]
}

/// Where an entity is: one of these, a mod's or an item entity; none for what has no place here.
pub fn position(rt: &Rt, e: V) -> Option<[f64; 3]> {
    if is_proxy(rt, e) {
        return Some(pos(rt, e));
    }
    crate::entities::position(rt, e)
}

/// The object for the game's entity `id` of type `kind` (as a hook names it), at `pos` when the
/// hook told it: a player is the runtime's server player standing there, an item entity
/// `entities.rs`' object, the rest one object each, brought up to date from the game. Null for a
/// player or item entity not found.
pub fn of(rt: &mut Rt, id: i32, kind: &str, pos: Option<[f64; 3]>) -> V {
    match kind {
        "minecraft:player" => return player_at(rt, pos),
        "minecraft:item" => return item(rt, id, pos),
        _ => {}
    }
    let view = rt.host().and_then(|h| h.entity(id));
    proxy(rt, id, kind, view.as_ref(), pos, false)
}

/// The object for the game's living entity `id` of type `kind` (not a player) where the game gives
/// no world to read (a hit's hooks): brought up to date from `view` when the game told it, else as
/// last seen. Of a living class whatever its type.
pub fn living(rt: &mut Rt, id: i32, kind: &str, view: Option<&EntityView>) -> V {
    proxy(rt, id, kind, view, None, true)
}

/// The high half of the UUID Horizon gives the game's entity of id `n` (`lan::entity_uuid`), whose
/// low half is `n`.
pub(crate) const UUID_HIGH: u64 = 0x4856_5a4e_0000_0000;

/// The object for the game's entity `id` (`Level.getEntity(int)`): a player by their entity id (on
/// a client, its own player by the id the server gave them, as a server's packet names them), any
/// other entity the level holds by its type. None when the game knows none (a client's own entities
/// aren't told).
pub(crate) fn by_id(rt: &mut Rt, id: i32) -> Option<V> {
    if rt.host()?.client_player_id() == Some(id) {
        return Some(crate::network::local_player(rt));
    }
    let held = rt.host()?.player_entities().into_iter().find(|p| p.0 == id);
    if let Some((id, uuid, at)) = held {
        return Some(player(rt, id, uuid, at));
    }
    let view = rt.host()?.entity(id)?;
    let v = of(rt, id, &view.kind, Some(view.pos));
    (v.r() != 0).then_some(v)
}

/// The server's player `uuid` as the level holds them: their object, with their entity id (which
/// tells them from the acting player: `attributes::entity_of`) and their place.
pub(crate) fn player(rt: &mut Rt, id: i32, uuid: u128, at: [f64; 3]) -> V {
    let p = crate::menus::server_player(rt, uuid);
    for (field, v) in ["$x", "$y", "$z"].into_iter().zip(at) {
        rt.put_field(p, field, V::D(v));
    }
    rt.put_field(p, "$entityId", V::I(id));
    p
}

/// The object the runtime holds for the game's entity `id`, if any.
pub fn known(rt: &mut Rt, id: i32) -> Option<V> {
    let list = proxies(rt);
    crate::jutil::elements(rt, list).into_iter().find(|&e| self::id(rt, e) == id && rt.get_field(e, "$alive").i() != 0)
}

/// Makes `e`, an entity of a vanilla class a mod made (`made_entities`), the object of the game's
/// entity `id` of type `kind`, which the game just made from it: the one object for that entity
/// from now on, brought up to date from the game. Its UUID is then the game's.
pub(crate) fn adopt(rt: &mut Rt, e: V, id: i32, kind: &str) {
    let now = game_time(rt);
    let list = proxies(rt);
    let mut items: Vec<V> = crate::jutil::elements(rt, list).into_iter().filter(|&o| self::id(rt, o) != id).collect();
    rt.put_field(e, "$vanilla", V::I(1));
    rt.put_field(e, "$vid", V::I(id));
    let k = rt.string(kind);
    rt.put_field(e, "$kind", k);
    rt.put_field(e, "$uuid", V::NULL);
    items.push(e);
    set_list(rt, list, items);
    let view = rt.host().and_then(|h| h.entity(id));
    refresh(rt, e, view.as_ref(), None);
    rt.put_field(e, "$seen", V::J(now));
}

/// An object standing for the game's entity `id` that the runtime hasn't seen and can't ask the
/// game about (a hit's attacker, where the game gives no world): of no type, a `LivingEntity` when
/// `living`, else an `Entity`. Not kept: the entity's own object comes once the game tells of it.
pub fn unseen(rt: &mut Rt, id: i32, living: bool) -> V {
    let e = rt.new_object(if living { LIVING } else { ENTITY });
    rt.put_field(e, "$vanilla", V::I(1));
    rt.put_field(e, "$vid", V::I(id));
    let k = rt.string("");
    rt.put_field(e, "$kind", k);
    rt.put_field(e, "$alive", V::I(1));
    e
}

fn proxy(rt: &mut Rt, id: i32, kind: &str, view: Option<&EntityView>, pos: Option<[f64; 3]>, living: bool) -> V {
    let now = game_time(rt);
    let list = proxies(rt);
    forget(rt, list, now);
    let known = crate::jutil::elements(rt, list).into_iter().find(|&e| self::id(rt, e) == id);
    let living = living || view.is_some_and(|v| v.health.is_some());
    let e = match known {
        Some(e) if rt.str_of(rt.get_field(e, "$kind")) == Some(kind) => e,
        _ => {
            // A new entity, or its id now another's (the old one went).
            let mut items: Vec<V> = crate::jutil::elements(rt, list).into_iter().filter(|&e| self::id(rt, e) != id).collect();
            if let Some(old) = known {
                rt.put_field(old, "$alive", V::I(0));
            }
            let e = rt.new_object(class_for(kind, living));
            rt.put_field(e, "$vanilla", V::I(1));
            rt.put_field(e, "$vid", V::I(id));
            let k = rt.string(kind);
            rt.put_field(e, "$kind", k);
            items.push(e);
            set_list(rt, list, items);
            e
        }
    };
    refresh(rt, e, view, pos);
    // Where the game gives no time (a hit's hooks), it was seen when it was last.
    if now != 0 || rt.get_field(e, "$seen").j() == 0 {
        rt.put_field(e, "$seen", V::J(now));
    }
    e
}

/// Brings `e` up to date from the game's `view` of its entity, else from where a hook said it is.
fn refresh(rt: &mut Rt, e: V, view: Option<&EntityView>, at: Option<[f64; 3]>) {
    if let Some(p) = view.map(|v| v.pos).or(at) {
        for (k, v) in ["$x", "$y", "$z"].into_iter().zip(p) {
            rt.put_field(e, k, V::D(v));
        }
    }
    if let Some(v) = view {
        rt.put_field(e, "$yRot", V::F(v.yaw));
        if let Some((health, max)) = v.health {
            rt.put_field(e, "$living", V::I(1));
            rt.put_field(e, "$health", V::F(health));
            rt.put_field(e, "$maxHealth", V::F(max));
        }
    }
    rt.put_field(e, "$alive", V::I(1));
}

/// Lets go of the objects whose entities the game no longer has and nothing asked for a while.
fn forget(rt: &mut Rt, list: V, now: i64) {
    let all = crate::jutil::elements(rt, list);
    let gone = |rt: &Rt, e: V| now - rt.get_field(e, "$seen").j() > FORGET_AFTER && rt.host().is_none_or(|h| h.entity(id(rt, e)).is_none());
    if all.iter().any(|&e| gone(rt, e)) {
        let keep = all.into_iter().filter(|&e| !gone(rt, e)).collect();
        set_list(rt, list, keep);
    }
}

/// The server player standing at `pos` (a hook's target), when the runtime's host knows them.
fn player_at(rt: &mut Rt, pos: Option<[f64; 3]>) -> V {
    let Some(p) = pos else {
        return V::NULL;
    };
    let players = rt.host().map(|h| h.players()).unwrap_or_default();
    let d = |q: [f64; 3]| (q[0] - p[0]).powi(2) + (q[1] - p[1]).powi(2) + (q[2] - p[2]).powi(2);
    match players.into_iter().min_by(|a, b| d(a.1).total_cmp(&d(b.1))) {
        Some((uuid, at)) if d(at) < 1e-4 => crate::menus::server_player(rt, uuid),
        _ => V::NULL,
    }
}

/// The item entity `id` near `pos`, as `entities.rs` gives it to mods.
fn item(rt: &mut Rt, id: i32, pos: Option<[f64; 3]>) -> V {
    let Some([x, y, z]) = pos else {
        return V::NULL;
    };
    let near = crate::entities::item_entities_in(rt, [x - 0.5, y - 0.5, z - 0.5, x + 0.5, y + 0.5, z + 0.5]);
    near.into_iter().find(|&e| rt.get_field(e, "$id").i() == id).unwrap_or(V::NULL)
}

/// The keys [`call`] may answer: any vanilla entity class's own (`Entity.getX`, `LivingEntity.hurt`,
/// `AbstractMinecart.getMinecartType`...), for one of these objects.
pub(crate) fn claims(owner: &str, _name: &str, _desc: &str) -> bool {
    class_entry(owner).is_some()
}

/// A write to the game's entity, which the game may answer by running the mod's hooks (a blow
/// fires the damage events): those get this runtime ([`Rt::host_call`]).
fn with_host<T>(rt: &mut Rt, f: impl FnOnce(&mut dyn Host) -> T) -> Option<T> {
    rt.host_call(f)
}

fn vec3(rt: &mut Rt, p: [f64; 3]) -> V {
    crate::api_common::vec3(rt, p[0], p[1], p[2])
}

fn xyz_of(rt: &Rt, v: V) -> [f64; 3] {
    ["x", "y", "z"].map(|k| rt.get_field(v, k).d())
}

/// What vanilla's entity classes answer for one of these objects.
pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (_, name, desc) = crate::parts(key)?;
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let e = a(0);
    if !is_proxy(rt, e) {
        return None;
    }
    let kind = rt.str_of(rt.get_field(e, "$kind")).unwrap_or("minecraft:pig").to_owned();
    let living = rt.get_field(e, "$living").i() != 0;
    let alive = rt.get_field(e, "$alive").i() != 0;
    let health = rt.get_field(e, "$health").f();
    let p = pos(rt, e);
    let (width, height, eye) = size(&kind, living);
    let id = id(rt, e);
    Some(match name {
        "getId" => V::I(id),
        "getType" => crate::mod_entities::vanilla_type(rt, &kind),
        "getX" => V::D(p[0]),
        "getY" => V::D(p[1]),
        "getZ" => V::D(p[2]),
        "getBlockX" | "getBlockY" | "getBlockZ" => V::I(p[(name.as_bytes()[8] - b'X') as usize].floor() as i32),
        "position" | "getPosition" => vec3(rt, p),
        "blockPosition" | "getOnPos" => {
            let y = if name == "getOnPos" { p[1] - 1.0e-5 } else { p[1] };
            api::block_pos(rt, p[0].floor() as i64, y.floor() as i64, p[2].floor() as i64)
        }
        "getYRot" | "getYHeadRot" | "getVisualRotationYInDegrees" => V::F(rt.get_field(e, "$yRot").f()),
        "getXRot" => V::F(0.0),
        // Where it looks: its facing, level (the game doesn't tell a mob's pitch).
        "getLookAngle" | "getViewVector" | "getForward" => crate::api_common::view_vector(rt, rt.get_field(e, "$yRot").f(), 0.0),
        // What last hurt it, as the hits the runtime was told of name it (`damage::hurt_by`): its
        // revenge target, and who gets the credit for a kill (`getKillCredit`: a player that hurt
        // it first).
        "getKillCredit" | "getLastHurtByMob" | "getLastAttacker" if living => {
            let player = if name == "getKillCredit" { rt.get_field(e, "$lastHurtByPlayer") } else { V::NULL };
            match (player, rt.get_field(e, "$lastHurtBy")) {
                (p, _) if p.r() != 0 => p,
                (_, by) if by.r() != 0 => by,
                _ => V::NULL,
            }
        }
        // The game's motion of it, in blocks a tick: still when the game can't tell.
        "getDeltaMovement" => {
            let m = rt
                .host()
                .and_then(|h| h.entity_motion(id))
                .unwrap_or_default();
            vec3(rt, m)
        }
        "onGround" => V::I(1),
        "getBoundingBox" => {
            let (hw, h) = (f64::from(width) / 2.0, f64::from(height));
            crate::shapes::aabb(rt, [p[0] - hw, p[1], p[2] - hw, p[0] + hw, p[1] + h, p[2] + hw])
        }
        "getBbWidth" => V::F(width),
        "getBbHeight" => V::F(height),
        "getEyeHeight" => V::F(eye),
        "getDimensions" | "getDefaultDimensions" if desc == POSE_DIMENSIONS => {
            pose_dimensions(rt, &kind, living, name, a(1))
        }
        // Its pose: the game doesn't tell a mob's, so standing (a player's is its own).
        "getPose" => {
            crate::api_common::plain_enum(rt, "net/minecraft/world/entity/Pose", "STANDING")
                .unwrap_or(V::NULL)
        }
        "getScale" | "getAgeScale" if living && desc == "()F" => V::F(1.0),
        "getEyeY" => V::D(p[1] + f64::from(eye)),
        "getEyePosition" => vec3(rt, [p[0], p[1] + f64::from(eye), p[2]]),
        "level" | "getCommandSenderWorld" => crate::glue::level(rt),
        // Its level's registries (Create's haunting hands them to `deserializeNBT`).
        "registryAccess" => {
            let level = crate::glue::level(rt);
            api::call(rt, "net/minecraft/world/level/Level.registryAccess.()Lnet/minecraft/core/RegistryAccess;", &[level])
        }
        "isAlive" => V::I(i32::from(alive && (!living || health > 0.0))),
        "isRemoved" => V::I(i32::from(!alive)),
        "isDeadOrDying" if living => V::I(i32::from(health <= 0.0)),
        "getHealth" if living => V::F(health),
        "getMaxHealth" if living => V::F(rt.get_field(e, "$maxHealth").f()),
        "getName" | "getDisplayName" | "getTypeName" => {
            let (ns, path) = kind.split_once(':').unwrap_or(("minecraft", kind.as_str()));
            let key = rt.string(&format!("entity.{ns}.{}", path.replace('/', ".")));
            crate::api_common::component(rt, key, true, V::NULL)
        }
        "getUUID" | "getStringUUID" => {
            if rt.get_field(e, "$uuid").r() == 0 {
                // `lan::entity_uuid`: Horizon's stable UUID for an entity by its id.
                let u = crate::tags::uuid(rt, UUID_HIGH as i64, i64::from(id as u32));
                rt.put_field(e, "$uuid", u);
            }
            let u = rt.get_field(e, "$uuid");
            if name == "getUUID" {
                u
            } else {
                let (msb, lsb) = crate::tags::uuid_bits(rt, u);
                rt.string(&crate::tags::uuid_text(msb, lsb))
            }
        }
        "getPersistentData" => crate::entities::persistent_data(rt, e),
        "distanceTo" | "distanceToSqr" => {
            let q = match desc {
                "(DDD)D" => [a(1).d(), a(2).d(), a(3).d()],
                "(Lnet/minecraft/world/phys/Vec3;)D" => xyz_of(rt, a(1)),
                _ => crate::api_common::entity_place(rt, a(1)).unwrap_or(p),
            };
            let d2 = (q[0] - p[0]).powi(2) + (q[1] - p[1]).powi(2) + (q[2] - p[2]).powi(2);
            if name == "distanceTo" { V::F(d2.sqrt() as f32) } else { V::D(d2) }
        }
        // What it does, through the game.
        // As vanilla's `hurt(source, amount)`, through the game: its frames, armor, knockback, the
        // flash and sound, the kill credited to the source's player or mob. The blow's causing
        // entity is what last hurt it.
        "hurt" | "hurtServer" => {
            let amount = a(args.len() - 1).f();
            let source = a(args.len() - 2);
            let blow = crate::damage::blow_of(rt, source);
            let hurt = crate::damage::dealing(rt, id, source, |rt| with_host(rt, |h| h.hurt_entity_by(id, amount, &blow))).unwrap_or(false);
            if Rt::tracing() {
                eprintln!("horizon-compat: {kind} {id} hurt by {amount} ({}): {}", blow.damage_type, if hurt { "taken" } else { "refused" });
            }
            if hurt && living {
                let by = rt.get_field(source, "$causing");
                crate::damage::hurt_by(rt, e, by);
            }
            if hurt {
                let view = rt.host().and_then(|h| h.entity(id));
                match view {
                    Some(v) => refresh(rt, e, Some(&v), None),
                    None => rt.put_field(e, "$alive", V::I(0)),
                }
            }
            V::I(i32::from(hurt))
        }
        "kill" => {
            if with_host(rt, |h| h.kill_entity(id)).unwrap_or(false) {
                rt.put_field(e, "$alive", V::I(0));
            }
            V::Void
        }
        "discard" | "remove" | "setRemoved" => {
            if with_host(rt, |h| h.remove_entity(id)).unwrap_or(false) {
                rt.put_field(e, "$alive", V::I(0));
            }
            V::Void
        }
        "setHealth" | "heal" if living => {
            let to = if name == "heal" { health + a(1).f() } else { a(1).f() };
            let max = rt.get_field(e, "$maxHealth").f();
            let to = to.min(max);
            if to <= 0.0 {
                if with_host(rt, |h| h.kill_entity(id)).unwrap_or(false) {
                    rt.put_field(e, "$alive", V::I(0));
                }
            } else if with_host(rt, |h| h.set_entity_health(id, to)).unwrap_or(false) {
                rt.put_field(e, "$health", V::F(to));
            }
            V::Void
        }
        // `LivingEntity.knockback(strength, x, z)`, as the game knocks a mob back: away from the
        // direction, less its knockback resistance.
        "knockback" if living && desc.starts_with("(DDD)") => {
            with_host(rt, |h| h.knockback_entity(id, a(1).d(), a(2).d(), a(3).d()));
            V::Void
        }
        // Whether a blow, the crosshair or a projectile finds it (`isPickable`), as 1.21.1's
        // classes answer: a living entity, a vehicle, falling block or lit TNT while it's there
        // (the game doesn't tell an armor stand that is a marker, which isn't), the dragon's parts
        // (not the dragon), a crystal, a hanging entity, an interaction, a shulker bullet; a
        // projectile of `#redirectable_projectile` but an arrow (whether one is stuck isn't told);
        // nothing else (an item, an orb, a cloud).
        "isPickable" => {
            let class = rt.class_of(e);
            let is = |c: &str| is_a(class, c);
            const E: &str = "net/minecraft/world/entity/";
            let pickable = if is("net/minecraft/world/entity/boss/enderdragon/EnderDragon") {
                false
            } else if ["LivingEntity", "item/FallingBlockEntity", "item/PrimedTnt", "vehicle/AbstractMinecart", "vehicle/Boat"].iter().any(|c| is(&format!("{E}{c}"))) {
                alive
            } else if ["boss/EnderDragonPart", "boss/enderdragon/EndCrystal", "decoration/BlockAttachedEntity", "Interaction", "projectile/ShulkerBullet"].iter().any(|c| is(&format!("{E}{c}"))) {
                true
            } else if is("net/minecraft/world/entity/projectile/Projectile") {
                !is("net/minecraft/world/entity/projectile/AbstractArrow") && rt.host().is_some_and(|h| h.entity_type_in_tag(&kind, "minecraft:redirectable_projectile"))
            } else {
                false
            };
            V::I(i32::from(pickable))
        }
        // The game keeps the motion: `setDeltaMovement` sets it, `push` and `addDeltaMovement` add to
        // it. Another entity's push (`push(Entity)`: two boxes apart) is the game's own.
        "push" if desc == "(Lnet/minecraft/world/entity/Entity;)V" => V::Void,
        "push" | "addDeltaMovement" | "setDeltaMovement" => {
            let v = if desc.starts_with("(DDD)") { [a(1).d(), a(2).d(), a(3).d()] } else { xyz_of(rt, a(1)) };
            if name == "setDeltaMovement" {
                with_host(rt, |h| h.set_entity_motion(id, v));
            } else {
                with_host(rt, |h| h.push_entity(id, v));
            }
            V::Void
        }
        // `move(MoverType, movement)`: the game moves it as its own motion does, with its
        // collisions (Create's belts carry what stands on them). Where it lands, it is.
        "move"
            if desc
                == "(Lnet/minecraft/world/entity/MoverType;Lnet/minecraft/world/phys/Vec3;)V" =>
        {
            let movement = xyz_of(rt, a(2));
            if let Some(at) = with_host(rt, |h| h.move_entity(id, movement)).flatten() {
                for (k, v) in ["$x", "$y", "$z"].into_iter().zip(at) {
                    rt.put_field(e, k, V::D(v));
                }
            }
            V::Void
        }
        // The game's physics sets where it stands from its own collisions, and sends its motion to
        // the players who see it as it changes (`hurtMarked` asks no more of it).
        "setOnGround" | "$hurtMarked" if desc.ends_with("V") => V::Void,
        // How high it steps: `STEP_HEIGHT`'s default for a living one (the game doesn't tell a
        // mob's own), none for the rest, as `Entity.maxUpStep` answers.
        "maxUpStep" => V::F(if living { 0.6 } else { 0.0 }),
        // A mob's walk inputs: the game doesn't tell them; still.
        "$xxa" | "$yya" | "$zza" if living => V::F(0.0),
        // What a piston, and so a belt, does with it: `IGNORE` for the kinds that override it so
        // (a cloud, a marker, an interaction, a display), `NORMAL` for the rest.
        "getPistonPushReaction" => {
            let ignored = matches!(
                kind.as_str(),
                "minecraft:area_effect_cloud"
                    | "minecraft:marker"
                    | "minecraft:interaction"
                    | "minecraft:block_display"
                    | "minecraft:item_display"
                    | "minecraft:text_display"
            );
            crate::blocks::push_reaction_named(rt, if ignored { "ignore" } else { "normal" })
        }
        "teleportTo" | "setPos" | "moveTo" | "absMoveTo" if desc.starts_with("(DDD") || desc.starts_with("(Lnet/minecraft/world/phys/Vec3;") => {
            let to = if desc.starts_with("(DDD") { [a(1).d(), a(2).d(), a(3).d()] } else { xyz_of(rt, a(1)) };
            if with_host(rt, |h| h.teleport_entity(id, to)).unwrap_or(false) {
                for (k, v) in ["$x", "$y", "$z"].into_iter().zip(to) {
                    rt.put_field(e, k, V::D(v));
                }
            }
            V::Void
        }
        "isSpectator" | "isInvisible" | "isOnFire" | "isPassenger" | "isVehicle" | "isShiftKeyDown" | "isCrouching" | "isSteppingCarefully" | "isSilent" | "isNoGravity"
        | "isInWater" | "isInLava" | "isInWaterOrRain" | "isUnderWater" | "isBaby" | "isSleeping" | "isUsingItem" | "isFallFlying" | "fireImmune"
        | "isSprinting" | "isSwimming" | "isInvulnerable" | "isCustomNameVisible" | "hasCustomName" => V::I(0),
        "isAttackable" | "canBeCollidedWith" | "isEffectiveAi" | "shouldBeSaved" => V::I(i32::from(name != "canBeCollidedWith")),
        "getVehicle" | "getControllingPassenger" | "getFirstPassenger" | "getCustomName" | "getTarget" => V::NULL,
        "getRootVehicle" => e,
        "getPassengers" => crate::jutil::new_list(rt, Vec::new()),
        // What it wears and holds, as the level saves it (`equipment`): a horse's armor (Create's
        // haunting drops it), a zombie's weapon.
        "getMainHandItem" | "getOffhandItem" | "getItemBySlot" | "getItemInHand" | "getBodyArmorItem" | "getWeaponItem" if living => {
            let slot = match name {
                "getMainHandItem" | "getWeaponItem" => Some("mainhand".to_owned()),
                "getOffhandItem" => Some("offhand".to_owned()),
                "getBodyArmorItem" => Some("body".to_owned()),
                // `InteractionHand`: the main hand first.
                "getItemInHand" => Some(if rt.get_field(a(1), "$ordinal").i() == 1 { "offhand" } else { "mainhand" }.to_owned()),
                _ => rt.str_of(rt.get_field(a(1), "$name")).map(str::to_ascii_lowercase),
            };
            let saved = slot.and_then(|slot| match with_host(rt, |h| h.entity_nbt(id)).flatten()?.remove("equipment")? {
                Nbt::Compound(mut eq) => match eq.remove(&slot)? {
                    Nbt::Compound(stack) => Some(stack),
                    _ => None,
                },
                _ => None,
            });
            match saved.and_then(|stack| crate::api_common::parse_stack(rt, &stack)) {
                Some(stack) => stack,
                None => crate::api_common::get_static(rt, "net/minecraft/world/item/ItemStack", "EMPTY").unwrap_or(V::NULL),
            }
        }
        "getMainHandItem" | "getOffhandItem" | "getItemBySlot" | "getItemInHand" | "getUseItem" | "getWeaponItem" | "getBodyArmorItem" => {
            crate::api_common::get_static(rt, "net/minecraft/world/item/ItemStack", "EMPTY").unwrap_or(V::NULL)
        }
        // As the level saves it (`saveWithoutId`, NeoForge's `serializeNBT` with the type's id),
        // with the mod data the runtime keeps for it (`NeoForgeData`).
        "saveWithoutId" | "serializeNBT" => {
            let mut nbt = with_host(rt, |h| h.entity_nbt(id)).flatten().unwrap_or_default();
            let data = rt.get_field(e, "$data");
            if data.r() != 0 {
                let d = crate::nbt::from_runtime(rt, data);
                if !d.is_empty() {
                    nbt.insert("NeoForgeData".into(), Nbt::Compound(d));
                }
            }
            if name == "serializeNBT" {
                nbt.insert("id".into(), Nbt::String(kind.clone()));
                crate::nbt::to_runtime(rt, &nbt)
            } else {
                let out = crate::nbt::to_runtime(rt, &nbt);
                crate::made_entities::merge(rt, a(1), out);
                a(1)
            }
        }
        // `spawnAtLocation(stack[, offsetY])`, or an item's: a new item entity at its feet (raised by
        // the offset), added to the level; none for an empty stack, or on a client.
        "spawnAtLocation" => {
            let stack = if desc.starts_with("(Lnet/minecraft/world/item/ItemStack;") {
                a(1)
            } else {
                let item = crate::api_common::item_like_name(rt, a(1));
                crate::api_common::stack(rt, &item, 1)
            };
            let offset = if desc.contains(";F)") {
                f64::from(a(2).f())
            } else if desc.contains(";I)") {
                f64::from(a(2).i())
            } else {
                0.0
            };
            if crate::api_common::stack_empty(rt, stack) || crate::neo::is_client(rt) {
                return Some(V::NULL);
            }
            crate::entities::spawn_at(rt, [p[0], p[1] + offset, p[2]], stack)
        }
        "getRemainingFireTicks" | "getAirSupply" | "getTicksFrozen" | "getArmorValue" => V::I(0),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::MemoryHost;

    #[test]
    fn the_tables_are_sorted_for_their_searches() {
        assert!(TYPES.windows(2).all(|w| w[0].0 < w[1].0));
        assert!(CLASSES.windows(2).all(|w| w[0].0 < w[1].0));
    }

    /// The game's entities' sizes in a pose (`getDimensions`), as 1.21.1's give them: a cow its
    /// type's (0.9 by 1.4, eyes at 1.3), asleep a fixed fifth of a block; a minecart its type's in
    /// any pose; a player its pose's (crouching 1.5 high with the eyes at 1.27, swimming 0.6, dying
    /// a fixed fifth with the eyes still at 1.62, and a pose it has no size for as standing).
    #[test]
    fn the_games_entities_are_their_types_size_in_a_pose() {
        const ENTITY_SIZE: &str = "net/minecraft/world/entity/Entity.getDimensions.(Lnet/minecraft/world/entity/Pose;)Lnet/minecraft/world/entity/EntityDimensions;";
        const LIVING_SIZE: &str = "net/minecraft/world/entity/LivingEntity.getDimensions.(Lnet/minecraft/world/entity/Pose;)Lnet/minecraft/world/entity/EntityDimensions;";
        const PLAYER_SIZE: &str = "net/minecraft/world/entity/player/Player.getDefaultDimensions.(Lnet/minecraft/world/entity/Pose;)Lnet/minecraft/world/entity/EntityDimensions;";
        let mut rt = Rt::new();
        crate::install();
        let mut host = MemoryHost::default();
        host.entities.push(EntityView {
            id: 7,
            kind: "minecraft:cow".into(),
            pos: [1.5, 64.0, -2.5],
            yaw: 90.0,
            health: Some((10.0, 10.0)),
        });
        host.entities.push(EntityView {
            id: 8,
            kind: "minecraft:minecart".into(),
            pos: [4.5, 64.0, 0.5],
            yaw: 0.0,
            health: None,
        });
        rt.with_host(&mut host, |rt| {
            let size = |rt: &mut Rt, key: &'static str, e: V, pose: V| {
                let d = api::call(rt, key, &[e, pose]);
                (
                    ["width", "height", "eyeHeight"].map(|f| rt.get_field(d, f).f()),
                    rt.get_field(d, "fixed").i() != 0,
                )
            };
            let standing = api::get_static(rt, "net/minecraft/world/entity/Pose.STANDING");
            let sleeping = api::get_static(rt, "net/minecraft/world/entity/Pose.SLEEPING");
            let crouching = api::get_static(rt, "net/minecraft/world/entity/Pose.CROUCHING");
            let swimming = api::get_static(rt, "net/minecraft/world/entity/Pose.SWIMMING");
            let dying = api::get_static(rt, "net/minecraft/world/entity/Pose.DYING");
            let jumping = api::get_static(rt, "net/minecraft/world/entity/Pose.LONG_JUMPING");
            let cow = of(rt, 7, "minecraft:cow", None);
            let cart = of(rt, 8, "minecraft:minecart", None);
            assert_eq!(
                size(rt, ENTITY_SIZE, cow, standing),
                ([0.9, 1.4, 1.3], false)
            );
            assert_eq!(
                size(rt, LIVING_SIZE, cow, sleeping),
                ([0.2, 0.2, 0.2], true)
            );
            assert_eq!(
                size(rt, ENTITY_SIZE, cart, sleeping),
                ([0.98, 0.7, 0.595], false)
            );
            let player = crate::menus::server_player(rt, 42);
            assert_eq!(
                size(rt, LIVING_SIZE, player, crouching),
                ([0.6, 1.5, 1.27], false)
            );
            assert_eq!(
                size(rt, LIVING_SIZE, player, swimming),
                ([0.6, 0.6, 0.4], false)
            );
            assert_eq!(
                size(rt, LIVING_SIZE, player, dying),
                ([0.2, 0.2, 1.62], true)
            );
            assert_eq!(
                size(rt, PLAYER_SIZE, player, jumping),
                ([0.6, 1.8, 1.62], false)
            );
        });
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// A cow is an `Animal`, a `Mob`, a `LivingEntity` and an `Entity`, not an `Enemy`; a zombie
    /// is a `Monster` and an `Enemy`; a minecart an `AbstractMinecart`; 26.3's `oak_boat` a `Boat`.
    #[test]
    fn vanilla_entities_are_their_1_21_1_classes() {
        let cow = class_for("minecraft:cow", true);
        assert_eq!(cow, "net/minecraft/world/entity/animal/Cow");
        for family in ["net/minecraft/world/entity/animal/Animal", "net/minecraft/world/entity/Mob", "net/minecraft/world/entity/LivingEntity", ENTITY] {
            assert!(is_a(cow, family), "{family}");
        }
        assert!(!is_a(cow, "net/minecraft/world/entity/monster/Enemy"));
        let zombie = class_for("minecraft:zombie", true);
        assert!(is_a(zombie, "net/minecraft/world/entity/monster/Monster") && is_a(zombie, "net/minecraft/world/entity/monster/Enemy"));
        assert!(is_a(class_for("minecraft:minecart", false), "net/minecraft/world/entity/vehicle/AbstractMinecart"));
        assert!(is_a(class_for("minecraft:chest_minecart", false), "net/minecraft/world/Container"));
        assert_eq!(class_for("minecraft:oak_boat", false), BOAT);
        assert_eq!(class_for("minecraft:happy_ghast", true), PATHFINDER);
        assert_eq!(size("minecraft:zombie", true), (0.6, 1.95, 1.74));
    }

    /// A mod's code meets the game's cow: an `Animal` by `instanceof`, at the game's place, with
    /// its health; hurt, it loses health in the game, and a blow past its health kills it.
    #[test]
    fn a_mod_reads_and_hurts_the_games_entity() {
        let mut rt = Rt::new();
        crate::install();
        let mut host = MemoryHost::default();
        host.entities.push(EntityView { id: 7, kind: "minecraft:cow".into(), pos: [1.5, 64.0, -2.5], yaw: 90.0, health: Some((10.0, 10.0)) });
        host.entities.push(EntityView { id: 8, kind: "minecraft:minecart".into(), pos: [4.5, 64.0, 0.5], yaw: 0.0, health: None });
        host.entity_tags.insert("minecraft:arthropod".into(), vec!["minecraft:spider".into(), "minecraft:bee".into()]);
        rt.with_host(&mut host, |rt| {
            let cow = of(rt, 7, "minecraft:cow", None);
            assert_eq!(of(rt, 7, "minecraft:cow", None), cow, "one object for the entity");
            assert!(rt.instance_of(cow, "net/minecraft/world/entity/animal/Animal"));
            assert!(rt.instance_of(cow, "net/minecraft/world/entity/LivingEntity"));
            assert!(!rt.instance_of(cow, "net/minecraft/world/entity/monster/Enemy"));
            let x = api::call(rt, "net/minecraft/world/entity/Entity.getX.()D", &[cow]);
            assert_eq!(x.d(), 1.5);
            let pos = api::call(rt, "net/minecraft/world/entity/Entity.blockPosition.()Lnet/minecraft/core/BlockPos;", &[cow]);
            assert_eq!(api::pos_of(rt, pos), [1, 64, -3]);
            let ty = api::call(rt, "net/minecraft/world/entity/Entity.getType.()Lnet/minecraft/world/entity/EntityType;", &[cow]);
            assert_eq!(ty, api::get_static(rt, "net/minecraft/world/entity/EntityType.COW"));
            // Its type's tags, as the game holds them (Create's cardboard sword asks for arthropods).
            let is = "net/minecraft/world/entity/EntityType.is.(Lnet/minecraft/tags/TagKey;)Z";
            let arthropod = api::get_static(rt, "net/minecraft/tags/EntityTypeTags.ARTHROPOD");
            assert_eq!(api::call(rt, is, &[ty, arthropod]).i(), 0, "a cow isn't an arthropod");
            let spider = api::get_static(rt, "net/minecraft/world/entity/EntityType.SPIDER");
            assert_eq!(api::call(rt, is, &[spider, arthropod]).i(), 1, "a spider is");
            // How far a player standing at (1.5, 64, 1.5) is from it, both ways.
            let player = crate::menus::server_player(rt, 42);
            for (f, v) in [("$x", 1.5), ("$y", 64.0), ("$z", 1.5)] {
                rt.put_field(player, f, V::D(v));
            }
            let sqr = "net/minecraft/world/entity/Entity.distanceToSqr.(Lnet/minecraft/world/entity/Entity;)D";
            assert_eq!(api::call(rt, sqr, &[player, cow]).d(), 16.0);
            assert_eq!(api::call(rt, sqr, &[cow, player]).d(), 16.0);
            let to = "net/minecraft/world/entity/Entity.distanceTo.(Lnet/minecraft/world/entity/Entity;)F";
            assert_eq!(api::call(rt, to, &[player, cow]).f(), 4.0);
            let hurt = api::call(rt, "net/minecraft/world/entity/LivingEntity.hurt.(Lnet/minecraft/world/damagesource/DamageSource;F)Z", &[cow, V::NULL, V::F(4.0)]);
            assert_eq!(hurt.i(), 1);
            let health = api::call(rt, "net/minecraft/world/entity/LivingEntity.getHealth.()F", &[cow]);
            assert_eq!(health.f(), 6.0);
            api::call(rt, "net/minecraft/world/entity/LivingEntity.hurt.(Lnet/minecraft/world/damagesource/DamageSource;F)Z", &[cow, V::NULL, V::F(100.0)]);
            assert_eq!(api::call(rt, "net/minecraft/world/entity/Entity.isAlive.()Z", &[cow]).i(), 0, "killed");
            let cart = of(rt, 8, "minecraft:minecart", None);
            assert!(rt.instance_of(cart, "net/minecraft/world/entity/vehicle/AbstractMinecart"));
            assert!(!rt.instance_of(cart, "net/minecraft/world/entity/LivingEntity"));
            let b = api::call(rt, "net/minecraft/world/entity/Entity.getBoundingBox.()Lnet/minecraft/world/phys/AABB;", &[cart]);
            let b = crate::shapes::aabb_of(rt, b);
            assert!((b[3] - b[0] - 0.98).abs() < 1e-6 && (b[4] - b[1] - 0.7).abs() < 1e-6, "{b:?}");
            api::call(rt, "net/minecraft/world/entity/Entity.kill.()V", &[cart]);
        });
        assert!(host.entities.is_empty(), "the cow died and the minecart broke: {:?}", host.entities);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// A mod reads the game's motion of its mob and changes it as vanilla's `Entity` does:
    /// `setDeltaMovement` sets it, `push` and `addDeltaMovement` add to it, and the next read sees
    /// it; another entity's `push(Entity)` is the game's own. A player's is this side's: a server's
    /// player's by their UUID, a client's own player's; another player's on a client isn't moved
    /// there.
    #[test]
    fn a_mod_reads_and_changes_the_motion_of_the_games_mob_and_players() {
        const GET: &str =
            "net/minecraft/world/entity/Entity.getDeltaMovement.()Lnet/minecraft/world/phys/Vec3;";
        const SET: &str =
            "net/minecraft/world/entity/Entity.setDeltaMovement.(Lnet/minecraft/world/phys/Vec3;)V";
        const SET_XYZ: &str = "net/minecraft/world/entity/Entity.setDeltaMovement.(DDD)V";
        const PUSH: &str = "net/minecraft/world/entity/Entity.push.(DDD)V";
        const ADD: &str =
            "net/minecraft/world/entity/Entity.addDeltaMovement.(Lnet/minecraft/world/phys/Vec3;)V";
        const PUSH_BY: &str =
            "net/minecraft/world/entity/Entity.push.(Lnet/minecraft/world/entity/Entity;)V";
        let mut rt = Rt::new();
        crate::install();
        let read = |rt: &mut Rt, e: V| {
            let v = api::call(rt, GET, &[e]);
            xyz_of(rt, v)
        };
        let mut host = MemoryHost::default();
        host.entities.push(EntityView {
            id: 7,
            kind: "minecraft:pig".into(),
            pos: [0.5, 64.0, 0.5],
            yaw: 0.0,
            health: Some((10.0, 10.0)),
        });
        host.motions.insert(7, [0.0, -0.0784, 0.0]);
        let uuid = 0x42u128;
        host.players = vec![(uuid, [3.5, 64.0, 0.5])];
        rt.with_host(&mut host, |rt| {
            let pig = of(rt, 7, "minecraft:pig", None);
            assert_eq!(
                read(rt, pig),
                [0.0, -0.0784, 0.0],
                "the game's motion of it"
            );
            let up = crate::api_common::vec3(rt, 0.0, 0.42, 0.0);
            api::call(rt, SET, &[pig, up]);
            api::call(rt, PUSH, &[pig, V::D(0.25), V::D(0.0), V::D(0.0)]);
            let back = crate::api_common::vec3(rt, 0.0, 0.0, -0.5);
            api::call(rt, ADD, &[pig, back]);
            assert_eq!(read(rt, pig), [0.25, 0.42, -0.5]);
            let player = crate::menus::server_player(rt, uuid);
            api::call(rt, PUSH_BY, &[pig, player]);
            assert_eq!(
                read(rt, pig),
                [0.25, 0.42, -0.5],
                "another entity's push is the game's"
            );
            // A server's player: the server's copy of theirs, by UUID.
            assert_eq!(read(rt, player), [0.0; 3]);
            api::call(rt, SET_XYZ, &[player, V::D(0.1), V::D(0.0), V::D(0.0)]);
            api::call(rt, PUSH, &[player, V::D(0.0), V::D(0.2), V::D(0.0)]);
            assert_eq!(read(rt, player), [0.1, 0.2, 0.0]);
        });
        assert_eq!(host.motions[&7], [0.25, 0.42, -0.5]);
        assert_eq!(host.player_motions[&uuid], [0.1, 0.2, 0.0]);
        // A client: its own player's, whatever the host calls them; another player's object there
        // is left alone.
        let mut client = MemoryHost {
            client: true,
            client_player_id: Some(57),
            player_id: Some(0x99),
            ..MemoryHost::default()
        };
        client.player_motions.insert(0x99, [0.0, -0.0784, 0.0]);
        client.players = vec![(uuid, [3.5, 64.0, 0.5])];
        rt.with_host(&mut client, |rt| {
            let me = crate::network::local_player(rt);
            assert_eq!(read(rt, me), [0.0, -0.0784, 0.0]);
            let wind = crate::api_common::vec3(rt, 0.0, 0.0, 0.3);
            api::call(rt, ADD, &[me, wind]);
            assert_eq!(read(rt, me), [0.0, -0.0784, 0.3]);
            let other = crate::menus::server_player(rt, uuid);
            api::call(rt, SET_XYZ, &[other, V::D(1.0), V::D(1.0), V::D(1.0)]);
            assert_eq!(read(rt, other), [0.0; 3]);
        });
        assert_eq!(
            client.player_motions.get(&uuid),
            None,
            "another player's isn't the client's"
        );
        assert_eq!(client.player_motions[&0x99], [0.0, -0.0784, 0.3]);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// A mod moves the game's mob as `Entity.move(MoverType.SELF, movement)` does (Create's belt
    /// carries what stands on it): the game moves it, and its object is where that took it. What a
    /// belt asks of what it carries answers as 1.21.1's classes do: a living one steps as
    /// `STEP_HEIGHT`'s default says and anything else not at all, a mob's walk inputs are still,
    /// a piston (so a belt) ignores a marker and pushes a pig and a player. Marking a server's
    /// player's motion (`hurtMarked = true`) sends it to their client and spends the mark; a mob's
    /// mark, an unmark and a client's own mark tell nobody. A server's player isn't moved by
    /// `move` (their client moves them) and carries no passengers; they are invisible while the
    /// effect lasts.
    #[test]
    fn a_mod_moves_the_games_mob_and_marks_a_players_motion() {
        const MOVE: &str = "net/minecraft/world/entity/Entity.move.(Lnet/minecraft/world/entity/MoverType;Lnet/minecraft/world/phys/Vec3;)V";
        const MARK: &str = "net/minecraft/world/entity/Entity.$hurtMarked.(Z)V";
        const MARKED: &str = "net/minecraft/world/entity/Entity.$hurtMarked.()Z";
        const STEP: &str = "net/minecraft/world/entity/Entity.maxUpStep.()F";
        const REACTION: &str = "net/minecraft/world/entity/Entity.getPistonPushReaction.()Lnet/minecraft/world/level/material/PushReaction;";
        let mut rt = Rt::new();
        crate::install();
        let named = |rt: &mut Rt, v: V| {
            let n = api::call(rt, "java/lang/Enum.name.()Ljava/lang/String;", &[v]);
            rt.str_of(n).unwrap_or_default().to_owned()
        };
        let mut host = MemoryHost::default();
        host.entities.push(EntityView {
            id: 7,
            kind: "minecraft:pig".into(),
            pos: [0.5, 64.0, 0.5],
            yaw: 0.0,
            health: Some((10.0, 10.0)),
        });
        host.entities.push(EntityView {
            id: 8,
            kind: "minecraft:marker".into(),
            pos: [2.5, 64.0, 0.5],
            yaw: 0.0,
            health: None,
        });
        let uuid = 0x42u128;
        host.player_id = Some(uuid);
        host.players = vec![(uuid, [3.5, 64.0, 0.5])];
        host.effects.insert(None, Vec::new());
        rt.with_host(&mut host, |rt| {
            let pig = of(rt, 7, "minecraft:pig", None);
            let along = crate::api_common::vec3(rt, 0.0, 0.0, 0.25);
            api::call(rt, MOVE, &[pig, V::NULL, along]);
            api::call(rt, MOVE, &[pig, V::NULL, along]);
            assert_eq!(
                api::call(rt, "net/minecraft/world/entity/Entity.getZ.()D", &[pig]).d(),
                1.0,
                "where the game moved it"
            );
            api::call(
                rt,
                "net/minecraft/world/entity/Entity.setOnGround.(Z)V",
                &[pig, V::I(1)],
            );
            assert_eq!(api::call(rt, STEP, &[pig]).f(), 0.6);
            assert_eq!(
                api::call(
                    rt,
                    "net/minecraft/world/entity/LivingEntity.$zza.()F",
                    &[pig]
                )
                .f(),
                0.0
            );
            let reaction = api::call(rt, REACTION, &[pig]);
            assert_eq!(named(rt, reaction), "NORMAL");
            let marker = of(rt, 8, "minecraft:marker", None);
            assert_eq!(api::call(rt, STEP, &[marker]).f(), 0.0);
            let reaction = api::call(rt, REACTION, &[marker]);
            assert_eq!(named(rt, reaction), "IGNORE");
            api::call(rt, MARK, &[pig, V::I(1)]);
            // The player: a write of the field keeps it, then tells the runtime (the translator's).
            let player = crate::menus::server_player(rt, uuid);
            api::call(rt, MOVE, &[player, V::NULL, along]);
            let riders = api::call(
                rt,
                "net/minecraft/world/entity/Entity.getPassengers.()Ljava/util/List;",
                &[player],
            );
            assert!(crate::jutil::elements(rt, riders).is_empty());
            let reaction = api::call(rt, REACTION, &[player]);
            assert_eq!(named(rt, reaction), "NORMAL");
            assert_eq!(api::call(rt, STEP, &[player]).f(), 0.6);
            rt.put_field(player, "hurtMarked", V::I(0));
            api::call(rt, MARK, &[player, V::I(0)]);
            rt.put_field(player, "hurtMarked", V::I(1));
            api::call(rt, MARK, &[player, V::I(1)]);
            assert_eq!(
                api::game_field(rt, player, "hurtMarked", "$hurtMarked", MARKED).i(),
                0,
                "the mark is spent"
            );
            let invisible = "net/minecraft/world/entity/Entity.isInvisible.()Z";
            assert_eq!(api::call(rt, invisible, &[player]).i(), 0);
            let effect = crate::host::EffectView {
                id: "minecraft:invisibility".into(),
                duration: 200,
                ..Default::default()
            };
            rt.host_call(|h| h.add_effect(None, &effect, false));
            assert_eq!(api::call(rt, invisible, &[player]).i(), 1);
        });
        assert_eq!(host.entities[0].pos, [0.5, 64.0, 1.0]);
        assert_eq!(
            host.players,
            vec![(uuid, [3.5, 64.0, 0.5])],
            "a server's player stays where their client said"
        );
        assert_eq!(host.marked_players, vec![uuid], "one mark sent");
        // A client's own player's mark tells nobody.
        let mut client = MemoryHost {
            client: true,
            client_player_id: Some(57),
            player_id: Some(0x99),
            ..MemoryHost::default()
        };
        rt.with_host(&mut client, |rt| {
            let me = crate::network::local_player(rt);
            rt.put_field(me, "hurtMarked", V::I(1));
            api::call(rt, MARK, &[me, V::I(1)]);
        });
        assert!(client.marked_players.is_empty());
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// A mod's blow tells the game its source as 1.21.1's `DamageSource` names it: the type, the
    /// player (by UUID) or the mob it is credited to and where it came from (that attacker's
    /// place); a source of none is generic. `knockback` pushes the living mob. The player keeps
    /// what they last hurt (a living entity only) and when. `isPickable` answers as 1.21.1's
    /// classes do: a mob, a minecart and an armor stand are; an orb and an arrow aren't; a
    /// fireball is as `#redirectable_projectile` holds it.
    #[test]
    fn a_mods_blow_names_its_source_and_its_knockback_pushes() {
        const HURT: &str = "net/minecraft/world/entity/LivingEntity.hurt.(Lnet/minecraft/world/damagesource/DamageSource;F)Z";
        const SET_LAST: &str = "net/minecraft/world/entity/player/Player.setLastHurtMob.(Lnet/minecraft/world/entity/Entity;)V";
        const LAST: &str = "net/minecraft/world/entity/player/Player.getLastHurtMob.()Lnet/minecraft/world/entity/LivingEntity;";
        const LAST_AT: &str = "net/minecraft/world/entity/player/Player.getLastHurtMobTimestamp.()I";
        let mut rt = Rt::new();
        crate::install();
        let mut host = MemoryHost::default();
        let view = |id: i32, kind: &str, pos: [f64; 3], health: Option<(f32, f32)>| EntityView { id, kind: kind.into(), pos, yaw: 0.0, health };
        host.entities.extend([
            view(7, "minecraft:cow", [1.5, 64.0, -2.5], Some((10.0, 10.0))),
            view(9, "minecraft:zombie", [4.5, 64.0, -2.5], Some((20.0, 20.0))),
            view(8, "minecraft:minecart", [4.5, 64.0, 0.5], None),
            view(10, "minecraft:armor_stand", [0.5, 64.0, 0.5], Some((20.0, 20.0))),
            view(11, "minecraft:experience_orb", [0.5, 64.0, 0.5], None),
            view(12, "minecraft:fireball", [0.5, 70.0, 0.5], None),
            view(13, "minecraft:arrow", [0.5, 70.0, 0.5], None),
        ]);
        host.entity_tags.insert("minecraft:redirectable_projectile".into(), vec!["minecraft:fireball".into(), "minecraft:wind_charge".into()]);
        let pickable = rt.with_host(&mut host, |rt| {
            let cow = of(rt, 7, "minecraft:cow", None);
            let player = crate::menus::server_player(rt, 42);
            for (f, v) in [("$x", 1.5), ("$y", 64.0), ("$z", 1.5)] {
                rt.put_field(player, f, V::D(v));
            }
            let by_player = crate::damage::source(rt, "minecraft:player_attack", player, player, V::NULL);
            assert_eq!(api::call(rt, HURT, &[cow, by_player, V::F(3.0)]), V::I(1));
            let zombie = of(rt, 9, "minecraft:zombie", None);
            let by_zombie = crate::damage::source(rt, "minecraft:mob_attack", zombie, zombie, V::NULL);
            assert_eq!(api::call(rt, HURT, &[cow, by_zombie, V::F(2.0)]), V::I(1));
            assert_eq!(api::call(rt, HURT, &[cow, V::NULL, V::F(1.0)]), V::I(1));
            api::call(rt, "net/minecraft/world/entity/LivingEntity.knockback.(DDD)V", &[cow, V::D(0.4), V::D(-1.0), V::D(0.0)]);

            rt.put_field(player, "tickCount", V::I(17));
            api::call(rt, SET_LAST, &[player, cow]);
            assert_eq!(api::call(rt, LAST, &[player]), cow);
            assert_eq!(api::call(rt, LAST_AT, &[player]), V::I(17));
            let cart = of(rt, 8, "minecraft:minecart", None);
            rt.put_field(player, "tickCount", V::I(18));
            api::call(rt, SET_LAST, &[player, cart]);
            assert_eq!(api::call(rt, LAST, &[player]), V::NULL, "a minecart isn't living");
            assert_eq!(api::call(rt, LAST_AT, &[player]), V::I(18));

            [(7, "minecraft:cow"), (8, "minecraft:minecart"), (10, "minecraft:armor_stand"), (11, "minecraft:experience_orb"), (12, "minecraft:fireball"), (13, "minecraft:arrow")].map(|(id, kind)| {
                let e = of(rt, id, kind, None);
                api::call(rt, "net/minecraft/world/entity/Entity.isPickable.()Z", &[e]).i()
            })
        });
        assert_eq!(pickable, [1, 1, 1, 0, 1, 0]);
        let blow = |damage_type: &str, player: Option<u128>, mob: Option<i32>, from: Option<[f64; 3]>| crate::host::HurtBy { damage_type: damage_type.into(), player, mob, from, motion: None, direct: None };
        assert_eq!(
            host.blows,
            vec![
                (7, 3.0, blow("minecraft:player_attack", Some(42), None, Some([1.5, 64.0, 1.5]))),
                (7, 2.0, blow("minecraft:mob_attack", None, Some(9), Some([4.5, 64.0, -2.5]))),
                (7, 1.0, blow("minecraft:generic", None, None, None)),
            ]
        );
        assert_eq!(host.entities.iter().find(|e| e.id == 7).and_then(|e| e.health), Some((4.0, 10.0)));
        assert_eq!(host.knockbacks, vec![(7, 0.4, -1.0, 0.0)]);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }
}
