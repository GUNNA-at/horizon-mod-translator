//! What living entities start with: vanilla's `AttributeSupplier` and its builder, the attribute
//! factories (`Mob.createMobAttributes()`, `Zombie.createAttributes()`...), `DefaultAttributes`, and
//! NeoForge's `EntityAttributeCreationEvent` and `EntityAttributeModificationEvent`, fired once the
//! registries are full, as NeoForge fires them (`CommonHooks.modifyAttributes`).
//!
//! A supplier or builder holds its attributes by registry id (`minecraft:generic.max_health`), each
//! with its holder and base value (the attribute's default where none was given). A value read
//! through a supplier is that base kept in the attribute's range, as an instance without modifiers
//! computes it. The suppliers mods give their types, or make by adding to a type's, are kept by
//! type id as NeoForge keeps them (`CommonHooks.FORGE_ATTRIBUTES`); vanilla's types answer from
//! 1.21.1's table. The game's own entities keep the game's attributes ([`crate::attributes`]).
use crate::{Data, Rt, V};

const SUPPLIER: &str = "net/minecraft/world/entity/ai/attributes/AttributeSupplier";
const BUILDER: &str = "net/minecraft/world/entity/ai/attributes/AttributeSupplier$Builder";
const DEFAULTS: &str = "net/minecraft/world/entity/ai/attributes/DefaultAttributes";
const CREATION: &str = "net/neoforged/neoforge/event/entity/EntityAttributeCreationEvent";
const MODIFICATION: &str = "net/neoforged/neoforge/event/entity/EntityAttributeModificationEvent";
const ATTRIBUTES: &str = "net/minecraft/world/entity/ai/attributes/Attributes";
const DESCRIPTION: &str =
    "net/minecraft/world/entity/EntityType.getDescriptionId.()Ljava/lang/String;";
/// A factory's descriptor: no arguments, a builder.
const FACTORY: &str = "()Lnet/minecraft/world/entity/ai/attributes/AttributeSupplier$Builder;";
/// What a supplier or builder holds of one attribute: its holder (`$holder`) and base (`$base`).
const ENTRY: &str = "compat/AttributeEntry";
/// The attributes a supplier or builder holds (`$instances`): entries by attribute id.
const INSTANCES: &str = "compat/AttributeInstances";
/// The suppliers mods gave their types, or made by adding to a type's, by type id.
const MOD_SUPPLIERS: &str = "compat:mod-attribute-suppliers";
const FIRED: &str = "compat:attribute-events-fired";

/// A factory: its class, its name, the factory it starts from (`class.name`), and what it adds.
type Factory = (
    &'static str,
    &'static str,
    &'static str,
    &'static [(&'static str, f64)],
);

/// 1.21.1's attribute factories (`public static AttributeSupplier.Builder ...()`, NeoForge's
/// additions included): the class and name of each, the factory it starts from (none for
/// `LivingEntity.createLivingAttributes`), and what it adds: an `Attributes` field or a NeoForge
/// attribute's id, with its base value (NaN for the attribute's default).
const FACTORIES: [Factory; 73] = [
    (
        "net/minecraft/world/entity/LivingEntity",
        "createLivingAttributes",
        "",
        &[
            ("MAX_HEALTH", f64::NAN),
            ("KNOCKBACK_RESISTANCE", f64::NAN),
            ("MOVEMENT_SPEED", f64::NAN),
            ("ARMOR", f64::NAN),
            ("ARMOR_TOUGHNESS", f64::NAN),
            ("MAX_ABSORPTION", f64::NAN),
            ("STEP_HEIGHT", f64::NAN),
            ("SCALE", f64::NAN),
            ("GRAVITY", f64::NAN),
            ("SAFE_FALL_DISTANCE", f64::NAN),
            ("FALL_DAMAGE_MULTIPLIER", f64::NAN),
            ("JUMP_STRENGTH", f64::NAN),
            ("OXYGEN_BONUS", f64::NAN),
            ("BURNING_TIME", f64::NAN),
            ("EXPLOSION_KNOCKBACK_RESISTANCE", f64::NAN),
            ("WATER_MOVEMENT_EFFICIENCY", f64::NAN),
            ("MOVEMENT_EFFICIENCY", f64::NAN),
            ("ATTACK_KNOCKBACK", f64::NAN),
            ("neoforge:swim_speed", f64::NAN),
            ("neoforge:nametag_distance", f64::NAN),
        ],
    ),
    (
        "net/minecraft/world/entity/Mob",
        "createMobAttributes",
        "net/minecraft/world/entity/LivingEntity.createLivingAttributes",
        &[("FOLLOW_RANGE", 16.0)],
    ),
    (
        "net/minecraft/world/entity/ambient/Bat",
        "createAttributes",
        "net/minecraft/world/entity/Mob.createMobAttributes",
        &[("MAX_HEALTH", 6.0)],
    ),
    (
        "net/minecraft/world/entity/animal/AbstractFish",
        "createAttributes",
        "net/minecraft/world/entity/Mob.createMobAttributes",
        &[("MAX_HEALTH", 3.0)],
    ),
    (
        "net/minecraft/world/entity/animal/Bee",
        "createAttributes",
        "net/minecraft/world/entity/Mob.createMobAttributes",
        &[
            ("MAX_HEALTH", 10.0),
            ("FLYING_SPEED", 0.6000000238418579),
            ("MOVEMENT_SPEED", 0.30000001192092896),
            ("ATTACK_DAMAGE", 2.0),
            ("FOLLOW_RANGE", 48.0),
        ],
    ),
    (
        "net/minecraft/world/entity/animal/Cat",
        "createAttributes",
        "net/minecraft/world/entity/Mob.createMobAttributes",
        &[
            ("MAX_HEALTH", 10.0),
            ("MOVEMENT_SPEED", 0.30000001192092896),
            ("ATTACK_DAMAGE", 3.0),
        ],
    ),
    (
        "net/minecraft/world/entity/animal/Chicken",
        "createAttributes",
        "net/minecraft/world/entity/Mob.createMobAttributes",
        &[("MAX_HEALTH", 4.0), ("MOVEMENT_SPEED", 0.25)],
    ),
    (
        "net/minecraft/world/entity/animal/Cow",
        "createAttributes",
        "net/minecraft/world/entity/Mob.createMobAttributes",
        &[
            ("MAX_HEALTH", 10.0),
            ("MOVEMENT_SPEED", 0.20000000298023224),
        ],
    ),
    (
        "net/minecraft/world/entity/animal/Dolphin",
        "createAttributes",
        "net/minecraft/world/entity/Mob.createMobAttributes",
        &[
            ("MAX_HEALTH", 10.0),
            ("MOVEMENT_SPEED", 1.2000000476837158),
            ("ATTACK_DAMAGE", 3.0),
        ],
    ),
    (
        "net/minecraft/world/entity/animal/Fox",
        "createAttributes",
        "net/minecraft/world/entity/Mob.createMobAttributes",
        &[
            ("MOVEMENT_SPEED", 0.30000001192092896),
            ("MAX_HEALTH", 10.0),
            ("FOLLOW_RANGE", 32.0),
            ("ATTACK_DAMAGE", 2.0),
            ("SAFE_FALL_DISTANCE", 5.0),
        ],
    ),
    (
        "net/minecraft/world/entity/animal/IronGolem",
        "createAttributes",
        "net/minecraft/world/entity/Mob.createMobAttributes",
        &[
            ("MAX_HEALTH", 100.0),
            ("MOVEMENT_SPEED", 0.25),
            ("KNOCKBACK_RESISTANCE", 1.0),
            ("ATTACK_DAMAGE", 15.0),
            ("STEP_HEIGHT", 1.0),
        ],
    ),
    (
        "net/minecraft/world/entity/animal/Ocelot",
        "createAttributes",
        "net/minecraft/world/entity/Mob.createMobAttributes",
        &[
            ("MAX_HEALTH", 10.0),
            ("MOVEMENT_SPEED", 0.30000001192092896),
            ("ATTACK_DAMAGE", 3.0),
        ],
    ),
    (
        "net/minecraft/world/entity/animal/Panda",
        "createAttributes",
        "net/minecraft/world/entity/Mob.createMobAttributes",
        &[
            ("MOVEMENT_SPEED", 0.15000000596046448),
            ("ATTACK_DAMAGE", 6.0),
        ],
    ),
    (
        "net/minecraft/world/entity/animal/Parrot",
        "createAttributes",
        "net/minecraft/world/entity/Mob.createMobAttributes",
        &[
            ("MAX_HEALTH", 6.0),
            ("FLYING_SPEED", 0.4000000059604645),
            ("MOVEMENT_SPEED", 0.20000000298023224),
            ("ATTACK_DAMAGE", 3.0),
        ],
    ),
    (
        "net/minecraft/world/entity/animal/Pig",
        "createAttributes",
        "net/minecraft/world/entity/Mob.createMobAttributes",
        &[("MAX_HEALTH", 10.0), ("MOVEMENT_SPEED", 0.25)],
    ),
    (
        "net/minecraft/world/entity/animal/PolarBear",
        "createAttributes",
        "net/minecraft/world/entity/Mob.createMobAttributes",
        &[
            ("MAX_HEALTH", 30.0),
            ("FOLLOW_RANGE", 20.0),
            ("MOVEMENT_SPEED", 0.25),
            ("ATTACK_DAMAGE", 6.0),
        ],
    ),
    (
        "net/minecraft/world/entity/animal/Rabbit",
        "createAttributes",
        "net/minecraft/world/entity/Mob.createMobAttributes",
        &[
            ("MAX_HEALTH", 3.0),
            ("MOVEMENT_SPEED", 0.30000001192092896),
            ("ATTACK_DAMAGE", 3.0),
        ],
    ),
    (
        "net/minecraft/world/entity/animal/Sheep",
        "createAttributes",
        "net/minecraft/world/entity/Mob.createMobAttributes",
        &[("MAX_HEALTH", 8.0), ("MOVEMENT_SPEED", 0.23000000417232513)],
    ),
    (
        "net/minecraft/world/entity/animal/SnowGolem",
        "createAttributes",
        "net/minecraft/world/entity/Mob.createMobAttributes",
        &[("MAX_HEALTH", 4.0), ("MOVEMENT_SPEED", 0.20000000298023224)],
    ),
    (
        "net/minecraft/world/entity/animal/Squid",
        "createAttributes",
        "net/minecraft/world/entity/Mob.createMobAttributes",
        &[("MAX_HEALTH", 10.0)],
    ),
    (
        "net/minecraft/world/entity/animal/Turtle",
        "createAttributes",
        "net/minecraft/world/entity/Mob.createMobAttributes",
        &[
            ("MAX_HEALTH", 30.0),
            ("MOVEMENT_SPEED", 0.25),
            ("STEP_HEIGHT", 1.0),
        ],
    ),
    (
        "net/minecraft/world/entity/animal/Wolf",
        "createAttributes",
        "net/minecraft/world/entity/Mob.createMobAttributes",
        &[
            ("MOVEMENT_SPEED", 0.30000001192092896),
            ("MAX_HEALTH", 8.0),
            ("ATTACK_DAMAGE", 4.0),
        ],
    ),
    (
        "net/minecraft/world/entity/animal/allay/Allay",
        "createAttributes",
        "net/minecraft/world/entity/Mob.createMobAttributes",
        &[
            ("MAX_HEALTH", 20.0),
            ("FLYING_SPEED", 0.10000000149011612),
            ("MOVEMENT_SPEED", 0.10000000149011612),
            ("ATTACK_DAMAGE", 2.0),
            ("FOLLOW_RANGE", 48.0),
        ],
    ),
    (
        "net/minecraft/world/entity/animal/armadillo/Armadillo",
        "createAttributes",
        "net/minecraft/world/entity/Mob.createMobAttributes",
        &[("MAX_HEALTH", 12.0), ("MOVEMENT_SPEED", 0.14)],
    ),
    (
        "net/minecraft/world/entity/animal/axolotl/Axolotl",
        "createAttributes",
        "net/minecraft/world/entity/Mob.createMobAttributes",
        &[
            ("MAX_HEALTH", 14.0),
            ("MOVEMENT_SPEED", 1.0),
            ("ATTACK_DAMAGE", 2.0),
            ("STEP_HEIGHT", 1.0),
        ],
    ),
    (
        "net/minecraft/world/entity/animal/camel/Camel",
        "createAttributes",
        "net/minecraft/world/entity/animal/horse/AbstractHorse.createBaseHorseAttributes",
        &[
            ("MAX_HEALTH", 32.0),
            ("MOVEMENT_SPEED", 0.09000000357627869),
            ("JUMP_STRENGTH", 0.41999998688697815),
            ("STEP_HEIGHT", 1.5),
        ],
    ),
    (
        "net/minecraft/world/entity/animal/frog/Frog",
        "createAttributes",
        "net/minecraft/world/entity/Mob.createMobAttributes",
        &[
            ("MOVEMENT_SPEED", 1.0),
            ("MAX_HEALTH", 10.0),
            ("ATTACK_DAMAGE", 10.0),
            ("STEP_HEIGHT", 1.0),
        ],
    ),
    (
        "net/minecraft/world/entity/animal/frog/Tadpole",
        "createAttributes",
        "net/minecraft/world/entity/Mob.createMobAttributes",
        &[("MOVEMENT_SPEED", 1.0), ("MAX_HEALTH", 6.0)],
    ),
    (
        "net/minecraft/world/entity/animal/goat/Goat",
        "createAttributes",
        "net/minecraft/world/entity/Mob.createMobAttributes",
        &[
            ("MAX_HEALTH", 10.0),
            ("MOVEMENT_SPEED", 0.20000000298023224),
            ("ATTACK_DAMAGE", 2.0),
        ],
    ),
    (
        "net/minecraft/world/entity/animal/horse/AbstractChestedHorse",
        "createBaseChestedHorseAttributes",
        "net/minecraft/world/entity/animal/horse/AbstractHorse.createBaseHorseAttributes",
        &[
            ("MOVEMENT_SPEED", 0.17499999701976776),
            ("JUMP_STRENGTH", 0.5),
        ],
    ),
    (
        "net/minecraft/world/entity/animal/horse/AbstractHorse",
        "createBaseHorseAttributes",
        "net/minecraft/world/entity/Mob.createMobAttributes",
        &[
            ("JUMP_STRENGTH", 0.7),
            ("MAX_HEALTH", 53.0),
            ("MOVEMENT_SPEED", 0.22499999403953552),
            ("STEP_HEIGHT", 1.0),
            ("SAFE_FALL_DISTANCE", 6.0),
            ("FALL_DAMAGE_MULTIPLIER", 0.5),
        ],
    ),
    (
        "net/minecraft/world/entity/animal/horse/Llama",
        "createAttributes",
        "net/minecraft/world/entity/animal/horse/AbstractChestedHorse.createBaseChestedHorseAttributes",
        &[("FOLLOW_RANGE", 40.0)],
    ),
    (
        "net/minecraft/world/entity/animal/horse/SkeletonHorse",
        "createAttributes",
        "net/minecraft/world/entity/animal/horse/AbstractHorse.createBaseHorseAttributes",
        &[
            ("MAX_HEALTH", 15.0),
            ("MOVEMENT_SPEED", 0.20000000298023224),
        ],
    ),
    (
        "net/minecraft/world/entity/animal/horse/ZombieHorse",
        "createAttributes",
        "net/minecraft/world/entity/animal/horse/AbstractHorse.createBaseHorseAttributes",
        &[
            ("MAX_HEALTH", 15.0),
            ("MOVEMENT_SPEED", 0.20000000298023224),
        ],
    ),
    (
        "net/minecraft/world/entity/animal/sniffer/Sniffer",
        "createAttributes",
        "net/minecraft/world/entity/Mob.createMobAttributes",
        &[
            ("MOVEMENT_SPEED", 0.10000000149011612),
            ("MAX_HEALTH", 14.0),
        ],
    ),
    (
        "net/minecraft/world/entity/boss/enderdragon/EnderDragon",
        "createAttributes",
        "net/minecraft/world/entity/Mob.createMobAttributes",
        &[("MAX_HEALTH", 200.0)],
    ),
    (
        "net/minecraft/world/entity/boss/wither/WitherBoss",
        "createAttributes",
        "net/minecraft/world/entity/monster/Monster.createMonsterAttributes",
        &[
            ("MAX_HEALTH", 300.0),
            ("MOVEMENT_SPEED", 0.6000000238418579),
            ("FLYING_SPEED", 0.6000000238418579),
            ("FOLLOW_RANGE", 40.0),
            ("ARMOR", 4.0),
        ],
    ),
    (
        "net/minecraft/world/entity/decoration/ArmorStand",
        "createAttributes",
        "net/minecraft/world/entity/LivingEntity.createLivingAttributes",
        &[("STEP_HEIGHT", 0.0)],
    ),
    (
        "net/minecraft/world/entity/monster/AbstractSkeleton",
        "createAttributes",
        "net/minecraft/world/entity/monster/Monster.createMonsterAttributes",
        &[("MOVEMENT_SPEED", 0.25)],
    ),
    (
        "net/minecraft/world/entity/monster/Blaze",
        "createAttributes",
        "net/minecraft/world/entity/monster/Monster.createMonsterAttributes",
        &[
            ("ATTACK_DAMAGE", 6.0),
            ("MOVEMENT_SPEED", 0.23000000417232513),
            ("FOLLOW_RANGE", 48.0),
        ],
    ),
    (
        "net/minecraft/world/entity/monster/Bogged",
        "createAttributes",
        "net/minecraft/world/entity/monster/AbstractSkeleton.createAttributes",
        &[("MAX_HEALTH", 16.0)],
    ),
    (
        "net/minecraft/world/entity/monster/CaveSpider",
        "createCaveSpider",
        "net/minecraft/world/entity/monster/Spider.createAttributes",
        &[("MAX_HEALTH", 12.0)],
    ),
    (
        "net/minecraft/world/entity/monster/Creeper",
        "createAttributes",
        "net/minecraft/world/entity/monster/Monster.createMonsterAttributes",
        &[("MOVEMENT_SPEED", 0.25)],
    ),
    (
        "net/minecraft/world/entity/monster/Drowned",
        "createAttributes",
        "net/minecraft/world/entity/monster/Zombie.createAttributes",
        &[("STEP_HEIGHT", 1.0)],
    ),
    (
        "net/minecraft/world/entity/monster/ElderGuardian",
        "createAttributes",
        "net/minecraft/world/entity/monster/Guardian.createAttributes",
        &[
            ("MOVEMENT_SPEED", 0.30000001192092896),
            ("ATTACK_DAMAGE", 8.0),
            ("MAX_HEALTH", 80.0),
        ],
    ),
    (
        "net/minecraft/world/entity/monster/EnderMan",
        "createAttributes",
        "net/minecraft/world/entity/monster/Monster.createMonsterAttributes",
        &[
            ("MAX_HEALTH", 40.0),
            ("MOVEMENT_SPEED", 0.30000001192092896),
            ("ATTACK_DAMAGE", 7.0),
            ("FOLLOW_RANGE", 64.0),
            ("STEP_HEIGHT", 1.0),
        ],
    ),
    (
        "net/minecraft/world/entity/monster/Endermite",
        "createAttributes",
        "net/minecraft/world/entity/monster/Monster.createMonsterAttributes",
        &[
            ("MAX_HEALTH", 8.0),
            ("MOVEMENT_SPEED", 0.25),
            ("ATTACK_DAMAGE", 2.0),
        ],
    ),
    (
        "net/minecraft/world/entity/monster/Evoker",
        "createAttributes",
        "net/minecraft/world/entity/monster/Monster.createMonsterAttributes",
        &[
            ("MOVEMENT_SPEED", 0.5),
            ("FOLLOW_RANGE", 12.0),
            ("MAX_HEALTH", 24.0),
        ],
    ),
    (
        "net/minecraft/world/entity/monster/Ghast",
        "createAttributes",
        "net/minecraft/world/entity/Mob.createMobAttributes",
        &[("MAX_HEALTH", 10.0), ("FOLLOW_RANGE", 100.0)],
    ),
    (
        "net/minecraft/world/entity/monster/Giant",
        "createAttributes",
        "net/minecraft/world/entity/monster/Monster.createMonsterAttributes",
        &[
            ("MAX_HEALTH", 100.0),
            ("MOVEMENT_SPEED", 0.5),
            ("ATTACK_DAMAGE", 50.0),
        ],
    ),
    (
        "net/minecraft/world/entity/monster/Guardian",
        "createAttributes",
        "net/minecraft/world/entity/monster/Monster.createMonsterAttributes",
        &[
            ("ATTACK_DAMAGE", 6.0),
            ("MOVEMENT_SPEED", 0.5),
            ("FOLLOW_RANGE", 16.0),
            ("MAX_HEALTH", 30.0),
        ],
    ),
    (
        "net/minecraft/world/entity/monster/Illusioner",
        "createAttributes",
        "net/minecraft/world/entity/monster/Monster.createMonsterAttributes",
        &[
            ("MOVEMENT_SPEED", 0.5),
            ("FOLLOW_RANGE", 18.0),
            ("MAX_HEALTH", 32.0),
        ],
    ),
    (
        "net/minecraft/world/entity/monster/MagmaCube",
        "createAttributes",
        "net/minecraft/world/entity/monster/Monster.createMonsterAttributes",
        &[("MOVEMENT_SPEED", 0.20000000298023224)],
    ),
    (
        "net/minecraft/world/entity/monster/Monster",
        "createMonsterAttributes",
        "net/minecraft/world/entity/Mob.createMobAttributes",
        &[("ATTACK_DAMAGE", f64::NAN)],
    ),
    (
        "net/minecraft/world/entity/monster/Pillager",
        "createAttributes",
        "net/minecraft/world/entity/monster/Monster.createMonsterAttributes",
        &[
            ("MOVEMENT_SPEED", 0.3499999940395355),
            ("MAX_HEALTH", 24.0),
            ("ATTACK_DAMAGE", 5.0),
            ("FOLLOW_RANGE", 32.0),
        ],
    ),
    (
        "net/minecraft/world/entity/monster/Ravager",
        "createAttributes",
        "net/minecraft/world/entity/monster/Monster.createMonsterAttributes",
        &[
            ("MAX_HEALTH", 100.0),
            ("MOVEMENT_SPEED", 0.3),
            ("KNOCKBACK_RESISTANCE", 0.75),
            ("ATTACK_DAMAGE", 12.0),
            ("ATTACK_KNOCKBACK", 1.5),
            ("FOLLOW_RANGE", 32.0),
            ("STEP_HEIGHT", 1.0),
        ],
    ),
    (
        "net/minecraft/world/entity/monster/Shulker",
        "createAttributes",
        "net/minecraft/world/entity/Mob.createMobAttributes",
        &[("MAX_HEALTH", 30.0)],
    ),
    (
        "net/minecraft/world/entity/monster/Silverfish",
        "createAttributes",
        "net/minecraft/world/entity/monster/Monster.createMonsterAttributes",
        &[
            ("MAX_HEALTH", 8.0),
            ("MOVEMENT_SPEED", 0.25),
            ("ATTACK_DAMAGE", 1.0),
        ],
    ),
    (
        "net/minecraft/world/entity/monster/Spider",
        "createAttributes",
        "net/minecraft/world/entity/monster/Monster.createMonsterAttributes",
        &[
            ("MAX_HEALTH", 16.0),
            ("MOVEMENT_SPEED", 0.30000001192092896),
        ],
    ),
    (
        "net/minecraft/world/entity/monster/Strider",
        "createAttributes",
        "net/minecraft/world/entity/Mob.createMobAttributes",
        &[
            ("MOVEMENT_SPEED", 0.17499999701976776),
            ("FOLLOW_RANGE", 16.0),
        ],
    ),
    (
        "net/minecraft/world/entity/monster/Vex",
        "createAttributes",
        "net/minecraft/world/entity/monster/Monster.createMonsterAttributes",
        &[("MAX_HEALTH", 14.0), ("ATTACK_DAMAGE", 4.0)],
    ),
    (
        "net/minecraft/world/entity/monster/Vindicator",
        "createAttributes",
        "net/minecraft/world/entity/monster/Monster.createMonsterAttributes",
        &[
            ("MOVEMENT_SPEED", 0.3499999940395355),
            ("FOLLOW_RANGE", 12.0),
            ("MAX_HEALTH", 24.0),
            ("ATTACK_DAMAGE", 5.0),
        ],
    ),
    (
        "net/minecraft/world/entity/monster/Witch",
        "createAttributes",
        "net/minecraft/world/entity/monster/Monster.createMonsterAttributes",
        &[("MAX_HEALTH", 26.0), ("MOVEMENT_SPEED", 0.25)],
    ),
    (
        "net/minecraft/world/entity/monster/Zoglin",
        "createAttributes",
        "net/minecraft/world/entity/monster/Monster.createMonsterAttributes",
        &[
            ("MAX_HEALTH", 40.0),
            ("MOVEMENT_SPEED", 0.30000001192092896),
            ("KNOCKBACK_RESISTANCE", 0.6000000238418579),
            ("ATTACK_KNOCKBACK", 1.0),
            ("ATTACK_DAMAGE", 6.0),
        ],
    ),
    (
        "net/minecraft/world/entity/monster/Zombie",
        "createAttributes",
        "net/minecraft/world/entity/monster/Monster.createMonsterAttributes",
        &[
            ("FOLLOW_RANGE", 35.0),
            ("MOVEMENT_SPEED", 0.23000000417232513),
            ("ATTACK_DAMAGE", 3.0),
            ("ARMOR", 2.0),
            ("SPAWN_REINFORCEMENTS_CHANCE", f64::NAN),
        ],
    ),
    (
        "net/minecraft/world/entity/monster/ZombifiedPiglin",
        "createAttributes",
        "net/minecraft/world/entity/monster/Zombie.createAttributes",
        &[
            ("SPAWN_REINFORCEMENTS_CHANCE", 0.0),
            ("MOVEMENT_SPEED", 0.23000000417232513),
            ("ATTACK_DAMAGE", 5.0),
        ],
    ),
    (
        "net/minecraft/world/entity/monster/breeze/Breeze",
        "createAttributes",
        "net/minecraft/world/entity/Mob.createMobAttributes",
        &[
            ("MOVEMENT_SPEED", 0.6299999952316284),
            ("MAX_HEALTH", 30.0),
            ("FOLLOW_RANGE", 24.0),
            ("ATTACK_DAMAGE", 3.0),
        ],
    ),
    (
        "net/minecraft/world/entity/monster/hoglin/Hoglin",
        "createAttributes",
        "net/minecraft/world/entity/monster/Monster.createMonsterAttributes",
        &[
            ("MAX_HEALTH", 40.0),
            ("MOVEMENT_SPEED", 0.30000001192092896),
            ("KNOCKBACK_RESISTANCE", 0.6000000238418579),
            ("ATTACK_KNOCKBACK", 1.0),
            ("ATTACK_DAMAGE", 6.0),
        ],
    ),
    (
        "net/minecraft/world/entity/monster/piglin/Piglin",
        "createAttributes",
        "net/minecraft/world/entity/monster/Monster.createMonsterAttributes",
        &[
            ("MAX_HEALTH", 16.0),
            ("MOVEMENT_SPEED", 0.3499999940395355),
            ("ATTACK_DAMAGE", 5.0),
        ],
    ),
    (
        "net/minecraft/world/entity/monster/piglin/PiglinBrute",
        "createAttributes",
        "net/minecraft/world/entity/monster/Monster.createMonsterAttributes",
        &[
            ("MAX_HEALTH", 50.0),
            ("MOVEMENT_SPEED", 0.3499999940395355),
            ("ATTACK_DAMAGE", 7.0),
        ],
    ),
    (
        "net/minecraft/world/entity/monster/warden/Warden",
        "createAttributes",
        "net/minecraft/world/entity/monster/Monster.createMonsterAttributes",
        &[
            ("MAX_HEALTH", 500.0),
            ("MOVEMENT_SPEED", 0.30000001192092896),
            ("KNOCKBACK_RESISTANCE", 1.0),
            ("ATTACK_KNOCKBACK", 1.5),
            ("ATTACK_DAMAGE", 30.0),
        ],
    ),
    (
        "net/minecraft/world/entity/npc/Villager",
        "createAttributes",
        "net/minecraft/world/entity/Mob.createMobAttributes",
        &[("MOVEMENT_SPEED", 0.5), ("FOLLOW_RANGE", 48.0)],
    ),
    (
        "net/minecraft/world/entity/player/Player",
        "createAttributes",
        "net/minecraft/world/entity/LivingEntity.createLivingAttributes",
        &[
            ("ATTACK_DAMAGE", 1.0),
            ("MOVEMENT_SPEED", 0.10000000149011612),
            ("ATTACK_SPEED", f64::NAN),
            ("LUCK", f64::NAN),
            ("BLOCK_INTERACTION_RANGE", 4.5),
            ("ENTITY_INTERACTION_RANGE", 3.0),
            ("BLOCK_BREAK_SPEED", f64::NAN),
            ("SUBMERGED_MINING_SPEED", f64::NAN),
            ("SNEAKING_SPEED", f64::NAN),
            ("MINING_EFFICIENCY", f64::NAN),
            ("SWEEPING_DAMAGE_RATIO", f64::NAN),
            ("neoforge:creative_flight", f64::NAN),
        ],
    ),
];

/// 1.21.1's `DefaultAttributes`: each living entity type's attributes, by its factory, in the
/// entity type registry's order.
const VANILLA_TYPES: [(&str, &str); 84] = [
    (
        "minecraft:allay",
        "net/minecraft/world/entity/animal/allay/Allay.createAttributes",
    ),
    (
        "minecraft:armadillo",
        "net/minecraft/world/entity/animal/armadillo/Armadillo.createAttributes",
    ),
    (
        "minecraft:armor_stand",
        "net/minecraft/world/entity/decoration/ArmorStand.createAttributes",
    ),
    (
        "minecraft:axolotl",
        "net/minecraft/world/entity/animal/axolotl/Axolotl.createAttributes",
    ),
    (
        "minecraft:bat",
        "net/minecraft/world/entity/ambient/Bat.createAttributes",
    ),
    (
        "minecraft:bee",
        "net/minecraft/world/entity/animal/Bee.createAttributes",
    ),
    (
        "minecraft:blaze",
        "net/minecraft/world/entity/monster/Blaze.createAttributes",
    ),
    (
        "minecraft:bogged",
        "net/minecraft/world/entity/monster/Bogged.createAttributes",
    ),
    (
        "minecraft:breeze",
        "net/minecraft/world/entity/monster/breeze/Breeze.createAttributes",
    ),
    (
        "minecraft:camel",
        "net/minecraft/world/entity/animal/camel/Camel.createAttributes",
    ),
    (
        "minecraft:cat",
        "net/minecraft/world/entity/animal/Cat.createAttributes",
    ),
    (
        "minecraft:cave_spider",
        "net/minecraft/world/entity/monster/CaveSpider.createCaveSpider",
    ),
    (
        "minecraft:chicken",
        "net/minecraft/world/entity/animal/Chicken.createAttributes",
    ),
    (
        "minecraft:cod",
        "net/minecraft/world/entity/animal/AbstractFish.createAttributes",
    ),
    (
        "minecraft:cow",
        "net/minecraft/world/entity/animal/Cow.createAttributes",
    ),
    (
        "minecraft:creeper",
        "net/minecraft/world/entity/monster/Creeper.createAttributes",
    ),
    (
        "minecraft:dolphin",
        "net/minecraft/world/entity/animal/Dolphin.createAttributes",
    ),
    (
        "minecraft:donkey",
        "net/minecraft/world/entity/animal/horse/AbstractChestedHorse.createBaseChestedHorseAttributes",
    ),
    (
        "minecraft:drowned",
        "net/minecraft/world/entity/monster/Drowned.createAttributes",
    ),
    (
        "minecraft:elder_guardian",
        "net/minecraft/world/entity/monster/ElderGuardian.createAttributes",
    ),
    (
        "minecraft:ender_dragon",
        "net/minecraft/world/entity/boss/enderdragon/EnderDragon.createAttributes",
    ),
    (
        "minecraft:enderman",
        "net/minecraft/world/entity/monster/EnderMan.createAttributes",
    ),
    (
        "minecraft:endermite",
        "net/minecraft/world/entity/monster/Endermite.createAttributes",
    ),
    (
        "minecraft:evoker",
        "net/minecraft/world/entity/monster/Evoker.createAttributes",
    ),
    (
        "minecraft:fox",
        "net/minecraft/world/entity/animal/Fox.createAttributes",
    ),
    (
        "minecraft:frog",
        "net/minecraft/world/entity/animal/frog/Frog.createAttributes",
    ),
    (
        "minecraft:ghast",
        "net/minecraft/world/entity/monster/Ghast.createAttributes",
    ),
    (
        "minecraft:giant",
        "net/minecraft/world/entity/monster/Giant.createAttributes",
    ),
    (
        "minecraft:glow_squid",
        "net/minecraft/world/entity/animal/Squid.createAttributes",
    ),
    (
        "minecraft:goat",
        "net/minecraft/world/entity/animal/goat/Goat.createAttributes",
    ),
    (
        "minecraft:guardian",
        "net/minecraft/world/entity/monster/Guardian.createAttributes",
    ),
    (
        "minecraft:hoglin",
        "net/minecraft/world/entity/monster/hoglin/Hoglin.createAttributes",
    ),
    (
        "minecraft:horse",
        "net/minecraft/world/entity/animal/horse/AbstractHorse.createBaseHorseAttributes",
    ),
    (
        "minecraft:husk",
        "net/minecraft/world/entity/monster/Zombie.createAttributes",
    ),
    (
        "minecraft:illusioner",
        "net/minecraft/world/entity/monster/Illusioner.createAttributes",
    ),
    (
        "minecraft:iron_golem",
        "net/minecraft/world/entity/animal/IronGolem.createAttributes",
    ),
    (
        "minecraft:llama",
        "net/minecraft/world/entity/animal/horse/Llama.createAttributes",
    ),
    (
        "minecraft:magma_cube",
        "net/minecraft/world/entity/monster/MagmaCube.createAttributes",
    ),
    (
        "minecraft:mooshroom",
        "net/minecraft/world/entity/animal/Cow.createAttributes",
    ),
    (
        "minecraft:mule",
        "net/minecraft/world/entity/animal/horse/AbstractChestedHorse.createBaseChestedHorseAttributes",
    ),
    (
        "minecraft:ocelot",
        "net/minecraft/world/entity/animal/Ocelot.createAttributes",
    ),
    (
        "minecraft:panda",
        "net/minecraft/world/entity/animal/Panda.createAttributes",
    ),
    (
        "minecraft:parrot",
        "net/minecraft/world/entity/animal/Parrot.createAttributes",
    ),
    (
        "minecraft:phantom",
        "net/minecraft/world/entity/monster/Monster.createMonsterAttributes",
    ),
    (
        "minecraft:pig",
        "net/minecraft/world/entity/animal/Pig.createAttributes",
    ),
    (
        "minecraft:piglin",
        "net/minecraft/world/entity/monster/piglin/Piglin.createAttributes",
    ),
    (
        "minecraft:piglin_brute",
        "net/minecraft/world/entity/monster/piglin/PiglinBrute.createAttributes",
    ),
    (
        "minecraft:pillager",
        "net/minecraft/world/entity/monster/Pillager.createAttributes",
    ),
    (
        "minecraft:polar_bear",
        "net/minecraft/world/entity/animal/PolarBear.createAttributes",
    ),
    (
        "minecraft:pufferfish",
        "net/minecraft/world/entity/animal/AbstractFish.createAttributes",
    ),
    (
        "minecraft:rabbit",
        "net/minecraft/world/entity/animal/Rabbit.createAttributes",
    ),
    (
        "minecraft:ravager",
        "net/minecraft/world/entity/monster/Ravager.createAttributes",
    ),
    (
        "minecraft:salmon",
        "net/minecraft/world/entity/animal/AbstractFish.createAttributes",
    ),
    (
        "minecraft:sheep",
        "net/minecraft/world/entity/animal/Sheep.createAttributes",
    ),
    (
        "minecraft:shulker",
        "net/minecraft/world/entity/monster/Shulker.createAttributes",
    ),
    (
        "minecraft:silverfish",
        "net/minecraft/world/entity/monster/Silverfish.createAttributes",
    ),
    (
        "minecraft:skeleton",
        "net/minecraft/world/entity/monster/AbstractSkeleton.createAttributes",
    ),
    (
        "minecraft:skeleton_horse",
        "net/minecraft/world/entity/animal/horse/SkeletonHorse.createAttributes",
    ),
    (
        "minecraft:slime",
        "net/minecraft/world/entity/monster/Monster.createMonsterAttributes",
    ),
    (
        "minecraft:sniffer",
        "net/minecraft/world/entity/animal/sniffer/Sniffer.createAttributes",
    ),
    (
        "minecraft:snow_golem",
        "net/minecraft/world/entity/animal/SnowGolem.createAttributes",
    ),
    (
        "minecraft:spider",
        "net/minecraft/world/entity/monster/Spider.createAttributes",
    ),
    (
        "minecraft:squid",
        "net/minecraft/world/entity/animal/Squid.createAttributes",
    ),
    (
        "minecraft:stray",
        "net/minecraft/world/entity/monster/AbstractSkeleton.createAttributes",
    ),
    (
        "minecraft:strider",
        "net/minecraft/world/entity/monster/Strider.createAttributes",
    ),
    (
        "minecraft:tadpole",
        "net/minecraft/world/entity/animal/frog/Tadpole.createAttributes",
    ),
    (
        "minecraft:trader_llama",
        "net/minecraft/world/entity/animal/horse/Llama.createAttributes",
    ),
    (
        "minecraft:tropical_fish",
        "net/minecraft/world/entity/animal/AbstractFish.createAttributes",
    ),
    (
        "minecraft:turtle",
        "net/minecraft/world/entity/animal/Turtle.createAttributes",
    ),
    (
        "minecraft:vex",
        "net/minecraft/world/entity/monster/Vex.createAttributes",
    ),
    (
        "minecraft:villager",
        "net/minecraft/world/entity/npc/Villager.createAttributes",
    ),
    (
        "minecraft:vindicator",
        "net/minecraft/world/entity/monster/Vindicator.createAttributes",
    ),
    (
        "minecraft:wandering_trader",
        "net/minecraft/world/entity/Mob.createMobAttributes",
    ),
    (
        "minecraft:warden",
        "net/minecraft/world/entity/monster/warden/Warden.createAttributes",
    ),
    (
        "minecraft:witch",
        "net/minecraft/world/entity/monster/Witch.createAttributes",
    ),
    (
        "minecraft:wither",
        "net/minecraft/world/entity/boss/wither/WitherBoss.createAttributes",
    ),
    (
        "minecraft:wither_skeleton",
        "net/minecraft/world/entity/monster/AbstractSkeleton.createAttributes",
    ),
    (
        "minecraft:wolf",
        "net/minecraft/world/entity/animal/Wolf.createAttributes",
    ),
    (
        "minecraft:zoglin",
        "net/minecraft/world/entity/monster/Zoglin.createAttributes",
    ),
    (
        "minecraft:zombie",
        "net/minecraft/world/entity/monster/Zombie.createAttributes",
    ),
    (
        "minecraft:zombie_horse",
        "net/minecraft/world/entity/animal/horse/ZombieHorse.createAttributes",
    ),
    (
        "minecraft:zombie_villager",
        "net/minecraft/world/entity/monster/Zombie.createAttributes",
    ),
    (
        "minecraft:zombified_piglin",
        "net/minecraft/world/entity/monster/ZombifiedPiglin.createAttributes",
    ),
    (
        "minecraft:player",
        "net/minecraft/world/entity/player/Player.createAttributes",
    ),
];

/// The vanilla classes that inherit their `createAttributes()` (`Husk.createAttributes()` is
/// `Zombie`'s, as a static call through a subclass resolves).
const INHERITED: [(&str, &str); 14] = [
    (
        "net/minecraft/world/entity/GlowSquid",
        "net/minecraft/world/entity/animal/Squid",
    ),
    (
        "net/minecraft/world/entity/animal/AbstractSchoolingFish",
        "net/minecraft/world/entity/animal/AbstractFish",
    ),
    (
        "net/minecraft/world/entity/animal/Cod",
        "net/minecraft/world/entity/animal/AbstractFish",
    ),
    (
        "net/minecraft/world/entity/animal/MushroomCow",
        "net/minecraft/world/entity/animal/Cow",
    ),
    (
        "net/minecraft/world/entity/animal/Pufferfish",
        "net/minecraft/world/entity/animal/AbstractFish",
    ),
    (
        "net/minecraft/world/entity/animal/Salmon",
        "net/minecraft/world/entity/animal/AbstractFish",
    ),
    (
        "net/minecraft/world/entity/animal/TropicalFish",
        "net/minecraft/world/entity/animal/AbstractFish",
    ),
    (
        "net/minecraft/world/entity/animal/horse/TraderLlama",
        "net/minecraft/world/entity/animal/horse/Llama",
    ),
    (
        "net/minecraft/world/entity/monster/CaveSpider",
        "net/minecraft/world/entity/monster/Spider",
    ),
    (
        "net/minecraft/world/entity/monster/Husk",
        "net/minecraft/world/entity/monster/Zombie",
    ),
    (
        "net/minecraft/world/entity/monster/Skeleton",
        "net/minecraft/world/entity/monster/AbstractSkeleton",
    ),
    (
        "net/minecraft/world/entity/monster/Stray",
        "net/minecraft/world/entity/monster/AbstractSkeleton",
    ),
    (
        "net/minecraft/world/entity/monster/WitherSkeleton",
        "net/minecraft/world/entity/monster/AbstractSkeleton",
    ),
    (
        "net/minecraft/world/entity/monster/ZombieVillager",
        "net/minecraft/world/entity/monster/Zombie",
    ),
];

/// The keys [`call`] may answer.
pub(crate) fn claims(owner: &str, name: &str, desc: &str) -> bool {
    matches!(
        owner,
        SUPPLIER | BUILDER | DEFAULTS | CREATION | MODIFICATION
    ) || (desc == FACTORY && factory(owner, name).is_some())
}

/// The vanilla factory a static call of `owner.name()` runs: the class's own, the one it inherits
/// (`Husk.createAttributes()`), or the one class of `net/minecraft/world/entity/` that declares a
/// factory of that name (`Monster.createMobAttributes()` is `Mob`'s).
fn factory(owner: &str, name: &str) -> Option<&'static Factory> {
    let owner = match INHERITED.iter().find(|(c, _)| *c == owner) {
        Some((_, from)) if name == "createAttributes" => from,
        _ => owner,
    };
    FACTORIES
        .iter()
        .find(|f| f.0 == owner && f.1 == name)
        .or_else(|| {
            if !owner.starts_with("net/minecraft/world/entity/") {
                return None;
            }
            let mut named = FACTORIES.iter().filter(|f| f.1 == name);
            let first = named.next()?;
            named.next().is_none().then_some(first)
        })
}

/// The builder factory `f` gives: the one it starts from, with its own attributes added.
fn run_factory(rt: &mut Rt, f: &Factory) -> V {
    let builder = match f
        .2
        .rsplit_once('.')
        .and_then(|(class, name)| FACTORIES.iter().find(|g| g.0 == class && g.1 == name))
    {
        Some(from) => run_factory(rt, from),
        None => new_builder(rt, V::NULL),
    };
    for &(attribute, base) in f.3 {
        let holder = if attribute.contains(':') {
            crate::attributes::attribute(rt, attribute)
        } else {
            crate::attributes::get_static(rt, ATTRIBUTES, attribute).unwrap_or(V::NULL)
        };
        add(rt, builder, holder, (!base.is_nan()).then_some(base));
    }
    builder
}

/// A new builder: empty, or holding what supplier `from` holds (`new Builder(supplier)`).
fn new_builder(rt: &mut Rt, from: V) -> V {
    let b = rt.new_object(BUILDER);
    let instances = copy_instances(rt, from);
    rt.put_field(b, "$instances", instances);
    b
}

/// A copy of the attributes supplier or builder `of` holds (none for null).
fn copy_instances(rt: &mut Rt, of: V) -> V {
    let held = match instances_of(rt, of) {
        Some(Data::Map(m)) => m.clone(),
        _ => Default::default(),
    };
    rt.alloc(INSTANCES, Data::Map(held))
}

fn instances_of(rt: &Rt, of: V) -> Option<&Data> {
    let m = if of.r() == 0 {
        V::NULL
    } else {
        rt.get_field(of, "$instances")
    };
    (m.r() != 0)
        .then(|| rt.heap.get(m.r() as usize).map(|o| &o.data))
        .flatten()
}

/// The entry supplier or builder `of` holds of attribute `id`.
fn entry(rt: &Rt, of: V, id: &str) -> Option<V> {
    match instances_of(rt, of) {
        Some(Data::Map(m)) => m.get(id).copied(),
        _ => None,
    }
}

/// `builder.add(attribute[, base])`: a new instance of the attribute, at `base` or its default. A
/// builder already built refuses a base other than the default, as the instance's callback
/// throws there (`Tried to change value for default attribute instance`), the instance kept.
fn add(rt: &mut Rt, builder: V, holder: V, base: Option<f64>) -> V {
    let Some(id) = attribute_key(rt, holder) else {
        return crate::jlang::throw_with(
            rt,
            "java/lang/NullPointerException",
            "Cannot invoke \"net.minecraft.core.Holder.value()\" because \"attribute\" is null",
        );
    };
    let default = crate::attributes::default(rt, holder);
    match base {
        Some(b) if b != default && rt.get_field(builder, "$frozen").i() != 0 => {
            put_entry(rt, builder, &id, holder, default);
            crate::jlang::throw_with(
                rt,
                "java/lang/UnsupportedOperationException",
                &format!("Tried to change value for default attribute instance: {id}"),
            )
        }
        _ => {
            put_entry(rt, builder, &id, holder, base.unwrap_or(default));
            builder
        }
    }
}

/// What a supplier keys attribute `holder` by: its registry id, or the holder itself for one of
/// no registry (`Holder.direct`); none for null.
fn attribute_key(rt: &mut Rt, holder: V) -> Option<String> {
    (holder.r() != 0).then(|| {
        crate::attributes::registry_id(rt, holder)
            .unwrap_or_else(|| format!("compat:holder#{}", holder.r()))
    })
}

fn put_entry(rt: &mut Rt, of: V, id: &str, holder: V, base: f64) {
    let e = rt.new_object(ENTRY);
    rt.put_field(e, "$holder", holder);
    rt.put_field(e, "$base", V::D(base));
    let mut m = rt.get_field(of, "$instances");
    if m.r() == 0 {
        m = rt.alloc(INSTANCES, Data::Map(Default::default()));
        rt.put_field(of, "$instances", m);
    }
    if let Some(crate::Object {
        data: Data::Map(map),
        ..
    }) = rt.heap.get_mut(m.r() as usize)
    {
        map.insert(id.to_owned(), e);
    }
}

/// `builder.combine(other)`: the other's attributes over this one's; built with it, the other is
/// built too.
fn combine(rt: &mut Rt, builder: V, other: V) {
    let theirs: Vec<(String, V)> = match instances_of(rt, other) {
        Some(Data::Map(m)) => m.iter().map(|(k, v)| (k.clone(), *v)).collect(),
        _ => Vec::new(),
    };
    for (id, e) in theirs {
        let (holder, base) = (rt.get_field(e, "$holder"), rt.get_field(e, "$base").d());
        put_entry(rt, builder, &id, holder, base);
    }
    let mut others = rt.get_field(builder, "$others");
    if others.r() == 0 {
        others = crate::jutil::new_list(rt, Vec::new());
        rt.put_field(builder, "$others", others);
    }
    if let Some(crate::Object {
        data: Data::List(list),
        ..
    }) = rt.heap.get_mut(others.r() as usize)
    {
        list.push(other);
    }
}

/// `builder.build()`: a supplier of what it holds; the builder (and those combined into it) frozen.
fn build(rt: &mut Rt, builder: V) -> V {
    rt.put_field(builder, "$frozen", V::I(1));
    for other in crate::jutil::elements(rt, rt.get_field(builder, "$others")) {
        rt.put_field(other, "$frozen", V::I(1));
    }
    let s = rt.new_object(SUPPLIER);
    let instances = copy_instances(rt, builder);
    rt.put_field(s, "$instances", instances);
    s
}

/// The id an entity type is registered under (vanilla's own for vanilla's).
fn type_id(rt: &Rt, ty: V) -> Option<String> {
    crate::mod_entities::type_id(rt, ty).map(|id| {
        if id.contains(':') {
            id.to_owned()
        } else {
            format!("minecraft:{id}")
        }
    })
}

/// `DefaultAttributes.getSupplier(type)`: the one a mod gave or made, else vanilla's, else null.
fn supplier_of(rt: &mut Rt, ty: V) -> V {
    match type_id(rt, ty) {
        Some(id) => supplier_by_id(rt, &id),
        None => V::NULL,
    }
}

fn supplier_by_id(rt: &mut Rt, id: &str) -> V {
    let mods = mod_suppliers(rt);
    if let Some(Data::Map(m)) = rt.heap.get(mods.r() as usize).map(|o| &o.data)
        && let Some(&s) = m.get(id)
    {
        return s;
    }
    let Some(&(_, key)) = VANILLA_TYPES.iter().find(|(t, _)| *t == id) else {
        return V::NULL;
    };
    rt.intern(format!("compat:vanilla-attribute-supplier/{id}"), |rt| {
        let (class, name) = key.rsplit_once('.').unwrap_or((key, ""));
        match FACTORIES.iter().find(|f| f.0 == class && f.1 == name) {
            Some(f) => {
                let b = run_factory(rt, f);
                build(rt, b)
            }
            None => V::NULL,
        }
    })
}

fn mod_suppliers(rt: &mut Rt) -> V {
    rt.intern(MOD_SUPPLIERS.to_owned(), |rt| {
        rt.alloc(INSTANCES, Data::Map(Default::default()))
    })
}

/// A supplier's read of attribute `holder`: its value (`getValue`) or base (`getBaseValue`), or
/// vanilla's `IllegalArgumentException` for an attribute it doesn't hold.
fn read(rt: &mut Rt, supplier: V, holder: V, base_only: bool) -> V {
    let id = attribute_key(rt, holder).unwrap_or_default();
    let Some(e) = entry(rt, supplier, &id) else {
        crate::jlang::throw_with(
            rt,
            "java/lang/IllegalArgumentException",
            &format!("Can't find attribute {id}"),
        );
        return V::D(0.0);
    };
    let base = rt.get_field(e, "$base").d();
    V::D(if base_only {
        base
    } else {
        crate::attributes::sanitize(rt, holder, base)
    })
}

/// Fires NeoForge's attribute events once, after the registries are full: mods give their types
/// suppliers, then add attributes to types that have one; each type added to gets its supplier
/// with the additions built over it.
pub(crate) fn fire(rt: &mut Rt) {
    if rt.interned.contains_key(FIRED) {
        return;
    }
    rt.interned.insert(FIRED.into(), V::I(1));
    let ev = rt.new_object(CREATION);
    crate::neo::post(rt, CREATION, ev);
    // The types with suppliers, as the event finds them in the registry: vanilla's in its order,
    // then the mods' by id.
    let mut types: Vec<V> = VANILLA_TYPES
        .iter()
        .map(|(id, _)| crate::mod_entities::vanilla_type(rt, id))
        .collect();
    let mods = mod_suppliers(rt);
    let mut ids: Vec<String> = match rt.heap.get(mods.r() as usize).map(|o| &o.data) {
        Some(Data::Map(m)) => m.keys().cloned().collect(),
        _ => Vec::new(),
    };
    ids.sort();
    for id in ids {
        if let Some(&t) = rt.registered.get(&("entity_type".to_owned(), id)) {
            types.push(t);
        }
    }
    let ev = rt.new_object(MODIFICATION);
    let list = crate::jutil::new_list(rt, types);
    rt.put_field(ev, "$types", list);
    let builders = rt.alloc(INSTANCES, Data::Map(Default::default()));
    rt.put_field(ev, "$builders", builders);
    crate::neo::post(rt, MODIFICATION, ev);
    let added: Vec<(String, V)> = match rt.heap.get(builders.r() as usize).map(|o| &o.data) {
        Some(Data::Map(m)) => m.iter().map(|(k, v)| (k.clone(), *v)).collect(),
        _ => Vec::new(),
    };
    for (id, additions) in added {
        let supplier = supplier_by_id(rt, &id);
        let b = new_builder(rt, supplier);
        combine(rt, b, additions);
        let built = build(rt, b);
        if let Some(crate::Object {
            data: Data::Map(m), ..
        }) = rt.heap.get_mut(mods.r() as usize)
        {
            m.insert(id, built);
        }
    }
}

pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, name, desc) = crate::parts(key)?;
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    if desc == FACTORY
        && let Some(f) = factory(owner, name)
    {
        return Some(run_factory(rt, f));
    }
    Some(match (owner, name) {
        (SUPPLIER, "builder") => new_builder(rt, V::NULL),
        (BUILDER, "<init>") => {
            let from = if desc == "()V" { V::NULL } else { a(1) };
            let instances = copy_instances(rt, from);
            rt.put_field(a(0), "$instances", instances);
            V::Void
        }
        (BUILDER, "add") => {
            let base = desc
                .starts_with("(Lnet/minecraft/core/Holder;D)")
                .then(|| a(2).d());
            add(rt, a(0), a(1), base)
        }
        (BUILDER, "build") => build(rt, a(0)),
        (BUILDER, "combine") => {
            combine(rt, a(0), a(1));
            V::Void
        }
        (BUILDER | SUPPLIER, "hasAttribute") => {
            let id = attribute_key(rt, a(1)).unwrap_or_default();
            V::I(i32::from(entry(rt, a(0), &id).is_some()))
        }
        (SUPPLIER, "getValue" | "getBaseValue") => read(rt, a(0), a(1), name == "getBaseValue"),
        // Nothing gives a supplier's attributes modifiers: a builder sets their bases only.
        (SUPPLIER, "hasModifier") => V::I(0),
        (SUPPLIER, "getModifierValue") => {
            let id = attribute_key(rt, a(1)).unwrap_or_default();
            let text = match entry(rt, a(0), &id) {
                Some(_) => format!(
                    "Can't find modifier {} on attribute {id}",
                    rt.str_of(a(2)).unwrap_or("null")
                ),
                None => format!("Can't find attribute {id}"),
            };
            crate::jlang::throw_with(rt, "java/lang/IllegalArgumentException", &text);
            V::D(0.0)
        }
        (DEFAULTS, "getSupplier") => supplier_of(rt, a(0)),
        (DEFAULTS, "hasSupplier") => {
            let s = supplier_of(rt, a(0));
            V::I(i32::from(s.r() != 0))
        }
        // Vanilla logs the living types without attributes; nothing to log here.
        (DEFAULTS, "validate") => V::Void,
        // A type has one supplier: a second (another mod's, or vanilla's type) is vanilla's error.
        (CREATION, "put") => {
            if supplier_of(rt, a(1)).r() != 0 {
                let named = crate::api::call(rt, DESCRIPTION, &[a(1)]);
                let named = rt.str_of(named).unwrap_or("null").to_owned();
                crate::jlang::throw_with(
                    rt,
                    "java/lang/IllegalStateException",
                    &format!("Duplicate DefaultAttributes entry: {named}"),
                );
                return Some(V::Void);
            }
            let Some(id) = type_id(rt, a(1)) else {
                return Some(V::Void);
            };
            let mods = mod_suppliers(rt);
            if let Some(crate::Object {
                data: Data::Map(m), ..
            }) = rt.heap.get_mut(mods.r() as usize)
            {
                m.insert(id, a(2));
            }
            V::Void
        }
        // `add(type, attribute[, base])`: onto the type's builder of additions (the attribute's
        // default where no base is given).
        (MODIFICATION, "add") => {
            let Some(id) = type_id(rt, a(1)) else {
                return Some(V::Void);
            };
            let builders = rt.get_field(a(0), "$builders");
            let mut b = match rt.heap.get(builders.r() as usize).map(|o| &o.data) {
                Some(Data::Map(m)) => m.get(&id).copied().unwrap_or(V::NULL),
                _ => V::NULL,
            };
            if b.r() == 0 {
                b = new_builder(rt, V::NULL);
                if let Some(crate::Object {
                    data: Data::Map(m), ..
                }) = rt.heap.get_mut(builders.r() as usize)
                {
                    m.insert(id, b);
                }
            }
            let base = if desc.contains(";D)") {
                a(3).d()
            } else {
                crate::attributes::default(rt, a(2))
            };
            add(rt, b, a(2), Some(base));
            V::Void
        }
        (MODIFICATION, "has") => {
            let attribute = attribute_key(rt, a(2)).unwrap_or_default();
            let supplier = supplier_of(rt, a(1));
            if supplier.r() == 0 {
                crate::jlang::throw_with(
                    rt,
                    "java/lang/NullPointerException",
                    "Cannot invoke \"net.minecraft.world.entity.ai.attributes.AttributeSupplier.hasAttribute(net.minecraft.core.Holder)\" because \"globalMap\" is null",
                );
                return Some(V::I(0));
            }
            let builders = rt.get_field(a(0), "$builders");
            let added = match (
                type_id(rt, a(1)),
                rt.heap.get(builders.r() as usize).map(|o| &o.data),
            ) {
                (Some(t), Some(Data::Map(m))) => m.get(&t).copied().unwrap_or(V::NULL),
                _ => V::NULL,
            };
            V::I(i32::from(
                entry(rt, supplier, &attribute).is_some() || entry(rt, added, &attribute).is_some(),
            ))
        }
        (MODIFICATION, "getTypes") => {
            let types = rt.get_field(a(0), "$types");
            if types.r() != 0 {
                types
            } else {
                crate::jutil::new_list(rt, Vec::new())
            }
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api;

    const BUILD: &str = "net/minecraft/world/entity/ai/attributes/AttributeSupplier$Builder.build.()Lnet/minecraft/world/entity/ai/attributes/AttributeSupplier;";
    const ADD: &str = "net/minecraft/world/entity/ai/attributes/AttributeSupplier$Builder.add.(Lnet/minecraft/core/Holder;D)Lnet/minecraft/world/entity/ai/attributes/AttributeSupplier$Builder;";
    const VALUE: &str = "net/minecraft/world/entity/ai/attributes/AttributeSupplier.getValue.(Lnet/minecraft/core/Holder;)D";
    const BASE: &str = "net/minecraft/world/entity/ai/attributes/AttributeSupplier.getBaseValue.(Lnet/minecraft/core/Holder;)D";
    const HAS: &str = "net/minecraft/world/entity/ai/attributes/AttributeSupplier.hasAttribute.(Lnet/minecraft/core/Holder;)Z";
    const GET_SUPPLIER: &str = "net/minecraft/world/entity/ai/attributes/DefaultAttributes.getSupplier.(Lnet/minecraft/world/entity/EntityType;)Lnet/minecraft/world/entity/ai/attributes/AttributeSupplier;";
    const PUT: &str = "net/neoforged/neoforge/event/entity/EntityAttributeCreationEvent.put.(Lnet/minecraft/world/entity/EntityType;Lnet/minecraft/world/entity/ai/attributes/AttributeSupplier;)V";
    const MODIFY_ADD: &str = "net/neoforged/neoforge/event/entity/EntityAttributeModificationEvent.add.(Lnet/minecraft/world/entity/EntityType;Lnet/minecraft/core/Holder;D)V";
    const MODIFY_ADD_DEFAULT: &str = "net/neoforged/neoforge/event/entity/EntityAttributeModificationEvent.add.(Lnet/minecraft/world/entity/EntityType;Lnet/minecraft/core/Holder;)V";
    const MODIFY_HAS: &str = "net/neoforged/neoforge/event/entity/EntityAttributeModificationEvent.has.(Lnet/minecraft/world/entity/EntityType;Lnet/minecraft/core/Holder;)Z";
    const TYPES: &str = "net/neoforged/neoforge/event/entity/EntityAttributeModificationEvent.getTypes.()Ljava/util/List;";

    fn attribute(rt: &mut Rt, field: &str) -> V {
        crate::attributes::get_static(rt, ATTRIBUTES, field).expect("an Attributes field")
    }

    /// A vanilla factory's builder, called through `owner`.
    fn made(rt: &mut Rt, owner: &str, name: &str) -> V {
        let key = rt.static_key(&format!("{owner}.{name}.{FACTORY}"));
        api::call(rt, key, &[])
    }

    fn values(rt: &mut Rt, supplier: V, fields: &[&str]) -> Vec<V> {
        fields
            .iter()
            .map(|f| {
                let h = attribute(rt, f);
                api::call(rt, VALUE, &[supplier, h])
            })
            .collect()
    }

    /// Vanilla's factories give its mobs 1.21.1's attributes (defaults where a factory names none,
    /// a float's base widened as Java widens it), through a subclass as the JVM resolves a static;
    /// `DefaultAttributes` gives each living type its supplier, none to an arrow; a base past the
    /// attribute's range reads in it, and an attribute a supplier lacks is vanilla's error.
    #[test]
    fn vanilla_factories_give_vanillas_attributes() {
        let mut rt = Rt::new();
        crate::install();
        let zombie = made(
            &mut rt,
            "net/minecraft/world/entity/monster/Zombie",
            "createAttributes",
        );
        let zombie = api::call(&mut rt, BUILD, &[zombie]);
        let fields = [
            "MAX_HEALTH",
            "FOLLOW_RANGE",
            "MOVEMENT_SPEED",
            "ATTACK_DAMAGE",
            "ARMOR",
            "SPAWN_REINFORCEMENTS_CHANCE",
        ];
        let zombies = [
            V::D(20.0),
            V::D(35.0),
            V::D(f64::from(0.23f32)),
            V::D(3.0),
            V::D(2.0),
            V::D(0.0),
        ];
        assert_eq!(values(&mut rt, zombie, &fields), zombies);
        // A husk's factory is the zombie's; so is its type's supplier, one object each time.
        let husk = made(
            &mut rt,
            "net/minecraft/world/entity/monster/Husk",
            "createAttributes",
        );
        let husk = api::call(&mut rt, BUILD, &[husk]);
        assert_eq!(values(&mut rt, husk, &fields), zombies);
        let husk_type = api::get_static(&mut rt, "net/minecraft/world/entity/EntityType.HUSK");
        let of_type = api::call(&mut rt, GET_SUPPLIER, &[husk_type]);
        assert_eq!(values(&mut rt, of_type, &fields), zombies);
        assert_eq!(api::call(&mut rt, GET_SUPPLIER, &[husk_type]), of_type);
        let arrow = api::get_static(&mut rt, "net/minecraft/world/entity/EntityType.ARROW");
        assert_eq!(api::call(&mut rt, GET_SUPPLIER, &[arrow]), V::NULL);
        let bee_type = api::get_static(&mut rt, "net/minecraft/world/entity/EntityType.BEE");
        let bee = api::call(&mut rt, GET_SUPPLIER, &[bee_type]);
        assert_eq!(
            values(&mut rt, bee, &["FLYING_SPEED", "FOLLOW_RANGE"]),
            [V::D(f64::from(0.6f32)), V::D(48.0)]
        );

        // Living entities have NeoForge's swim speed, at its default.
        let living = made(
            &mut rt,
            "net/minecraft/world/entity/LivingEntity",
            "createLivingAttributes",
        );
        let living = api::call(&mut rt, BUILD, &[living]);
        let swim = api::get_static(
            &mut rt,
            "net/neoforged/neoforge/common/NeoForgeMod.SWIM_SPEED",
        );
        assert_eq!(api::call(&mut rt, VALUE, &[living, swim]), V::D(1.0));

        // `Monster.createMobAttributes()` is `Mob`'s; a base past the range reads at its end.
        let mob = made(
            &mut rt,
            "net/minecraft/world/entity/monster/Monster",
            "createMobAttributes",
        );
        let health = attribute(&mut rt, "MAX_HEALTH");
        api::call(&mut rt, ADD, &[mob, health, V::D(5000.0)]);
        let mob = api::call(&mut rt, BUILD, &[mob]);
        assert_eq!(api::call(&mut rt, VALUE, &[mob, health]), V::D(1024.0));
        assert_eq!(api::call(&mut rt, BASE, &[mob, health]), V::D(5000.0));
        let follow = attribute(&mut rt, "FOLLOW_RANGE");
        assert_eq!(api::call(&mut rt, VALUE, &[mob, follow]), V::D(16.0));
        let flying = attribute(&mut rt, "FLYING_SPEED");
        assert_eq!(api::call(&mut rt, HAS, &[mob, flying]), V::I(0));
        assert_eq!(api::call(&mut rt, VALUE, &[mob, flying]), V::D(0.0));
        let e = rt.thrown.take().expect("an IllegalArgumentException");
        assert_eq!(rt.class_of(e), "java/lang/IllegalArgumentException");
        assert_eq!(
            rt.str_of(rt.get_field(e, "message")),
            Some("Can't find attribute minecraft:generic.flying_speed")
        );
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// NeoForge's events, as a mod's listeners use them: the creation event gives a mod's type its
    /// supplier and refuses a second one for a type that has one; the modification event adds to a
    /// mod's type and to vanilla's, lists the types with suppliers, and its additions are built
    /// over each type's supplier.
    #[test]
    fn mods_give_their_types_attributes_through_neoforges_events() {
        fn mods(rt: &mut Rt, id: u32, args: &[V]) -> Option<V> {
            let captured = rt.captured(args[0]);
            let (goblin, seen, event) = (captured[0], captured[1], args[1]);
            let zombie = api::get_static(rt, "net/minecraft/world/entity/EntityType.ZOMBIE");
            let mut heard = Vec::new();
            match id {
                1 => {
                    let b = made(
                        rt,
                        "net/minecraft/world/entity/monster/Monster",
                        "createMonsterAttributes",
                    );
                    let health = attribute(rt, "MAX_HEALTH");
                    api::call(rt, ADD, &[b, health, V::D(40.0)]);
                    let s = api::call(rt, BUILD, &[b]);
                    api::call(rt, PUT, &[event, goblin, s]);
                    api::call(rt, PUT, &[event, zombie, s]);
                    heard.push(rt.thrown.take().unwrap_or(V::NULL));
                }
                _ => {
                    let armor = attribute(rt, "ARMOR");
                    api::call(rt, MODIFY_ADD, &[event, goblin, armor, V::D(4.0)]);
                    let luck = attribute(rt, "LUCK");
                    api::call(rt, MODIFY_ADD_DEFAULT, &[event, zombie, luck]);
                    heard.push(api::call(rt, MODIFY_HAS, &[event, goblin, armor]));
                    heard.push(api::call(rt, TYPES, &[event]));
                }
            }
            for v in heard {
                if let Some(crate::Object {
                    data: Data::List(l),
                    ..
                }) = rt.heap.get_mut(seen.r() as usize)
                {
                    l.push(v);
                }
            }
            Some(V::Void)
        }
        let mut rt = Rt::new();
        crate::install();
        rt.lambda_call = Some(mods);
        let monster = api::get_static(&mut rt, "net/minecraft/world/entity/MobCategory.MONSTER");
        let of = "net/minecraft/world/entity/EntityType$Builder.of.(Lnet/minecraft/world/entity/EntityType$EntityFactory;Lnet/minecraft/world/entity/MobCategory;)Lnet/minecraft/world/entity/EntityType$Builder;";
        let builder = api::call(&mut rt, of, &[V::NULL, monster]);
        let name = rt.string("goblin");
        let goblin = api::call(
            &mut rt,
            "net/minecraft/world/entity/EntityType$Builder.build.(Ljava/lang/String;)Lnet/minecraft/world/entity/EntityType;",
            &[builder, name],
        );
        let registry = api::get_static(
            &mut rt,
            "net/minecraft/core/registries/BuiltInRegistries.ENTITY_TYPE",
        );
        let id = rt.string("test:goblin");
        api::call(
            &mut rt,
            "net/minecraft/core/Registry.register.(Lnet/minecraft/core/Registry;Lnet/minecraft/resources/ResourceLocation;Ljava/lang/Object;)Ljava/lang/Object;",
            &[registry, id, goblin],
        );
        let seen = crate::jutil::new_list(&mut rt, Vec::new());
        for (event, id) in [(CREATION, 1), (MODIFICATION, 2)] {
            let listener = rt.lambda_id("java/util/function/Consumer", id, vec![goblin, seen]);
            rt.listeners.push((event.to_owned(), listener));
        }
        fire(&mut rt);
        fire(&mut rt);

        let heard = crate::jutil::elements(&rt, seen);
        assert_eq!(heard.len(), 3, "each listener heard its event once");
        assert_eq!(rt.class_of(heard[0]), "java/lang/IllegalStateException");
        assert_eq!(
            rt.str_of(rt.get_field(heard[0], "message")),
            Some("Duplicate DefaultAttributes entry: entity.minecraft.zombie")
        );
        assert_eq!(heard[1], V::I(1));
        let types = crate::jutil::elements(&rt, heard[2]);
        let zombie = api::get_static(&mut rt, "net/minecraft/world/entity/EntityType.ZOMBIE");
        assert_eq!(types.len(), VANILLA_TYPES.len() + 1);
        assert!(types.contains(&zombie) && types.last() == Some(&goblin));

        let goblins = api::call(&mut rt, GET_SUPPLIER, &[goblin]);
        assert_eq!(
            values(
                &mut rt,
                goblins,
                &["MAX_HEALTH", "ARMOR", "ATTACK_DAMAGE", "FOLLOW_RANGE"]
            ),
            [V::D(40.0), V::D(4.0), V::D(2.0), V::D(16.0)]
        );
        let zombies = api::call(&mut rt, GET_SUPPLIER, &[zombie]);
        assert_eq!(
            values(&mut rt, zombies, &["LUCK", "FOLLOW_RANGE"]),
            [V::D(0.0), V::D(35.0)]
        );
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }
}
