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

use crate::host::{EntityView, Host};
use crate::{Rt, V, api};

const ENTITY: &str = "net/minecraft/world/entity/Entity";
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

fn id(rt: &Rt, e: V) -> i32 {
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
    let now = game_time(rt);
    let list = proxies(rt);
    forget(rt, list, now);
    let known = crate::jutil::elements(rt, list).into_iter().find(|&e| self::id(rt, e) == id);
    let living = view.as_ref().is_some_and(|v| v.health.is_some());
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
    refresh(rt, e, view.as_ref(), pos);
    rt.put_field(e, "$seen", V::J(now));
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

fn with_host<T>(rt: &mut Rt, f: impl FnOnce(&mut dyn Host) -> T) -> Option<T> {
    rt.host_mut().map(f)
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
        "position" => vec3(rt, p),
        "blockPosition" | "getOnPos" => {
            let y = if name == "getOnPos" { p[1] - 1.0e-5 } else { p[1] };
            api::block_pos(rt, p[0].floor() as i64, y.floor() as i64, p[2].floor() as i64)
        }
        "getYRot" | "getYHeadRot" | "getVisualRotationYInDegrees" => V::F(rt.get_field(e, "$yRot").f()),
        "getXRot" => V::F(0.0),
        "getDeltaMovement" => vec3(rt, [0.0; 3]),
        "onGround" => V::I(1),
        "getBoundingBox" => {
            let (hw, h) = (f64::from(width) / 2.0, f64::from(height));
            crate::shapes::aabb(rt, [p[0] - hw, p[1], p[2] - hw, p[0] + hw, p[1] + h, p[2] + hw])
        }
        "getBbWidth" => V::F(width),
        "getBbHeight" => V::F(height),
        "getEyeHeight" => V::F(eye),
        "getEyeY" => V::D(p[1] + f64::from(eye)),
        "getEyePosition" => vec3(rt, [p[0], p[1] + f64::from(eye), p[2]]),
        "level" | "getCommandSenderWorld" => crate::glue::level(rt),
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
                let u = crate::tags::uuid(rt, 0x4856_5a4e_0000_0000_u64 as i64, i64::from(id as u32));
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
        "hurt" | "hurtServer" => {
            let amount = a(args.len() - 1).f();
            let hurt = with_host(rt, |h| h.hurt_entity(id, amount)).unwrap_or(false);
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
        // The game keeps the motion; these add to it (a mob's own is not read yet).
        "push" | "addDeltaMovement" | "setDeltaMovement" => {
            let v = if desc.starts_with("(DDD)") { [a(1).d(), a(2).d(), a(3).d()] } else { xyz_of(rt, a(1)) };
            with_host(rt, |h| h.push_entity(id, v));
            V::Void
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
        "isSpectator" | "isInvisible" | "isOnFire" | "isPassenger" | "isVehicle" | "isShiftKeyDown" | "isCrouching" | "isSilent" | "isNoGravity"
        | "isInWater" | "isInLava" | "isInWaterOrRain" | "isUnderWater" | "isBaby" | "isSleeping" | "isUsingItem" | "isFallFlying" | "fireImmune"
        | "isSprinting" | "isSwimming" | "isInvulnerable" | "hasEffect" | "addEffect" | "removeEffect" | "isCustomNameVisible" | "hasCustomName" => V::I(0),
        "isAttackable" | "isPickable" | "canBeCollidedWith" | "isEffectiveAi" | "shouldBeSaved" => V::I(i32::from(name != "canBeCollidedWith")),
        "getEffect" | "getVehicle" | "getControllingPassenger" | "getFirstPassenger" | "getCustomName" | "getTarget" | "getLastHurtByMob" => V::NULL,
        "getRootVehicle" => e,
        "getPassengers" | "getActiveEffects" => crate::jutil::new_list(rt, Vec::new()),
        "getMainHandItem" | "getOffhandItem" | "getItemBySlot" | "getItemInHand" | "getUseItem" | "getWeaponItem" => {
            crate::api_common::get_static(rt, "net/minecraft/world/item/ItemStack", "EMPTY").unwrap_or(V::NULL)
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
}
