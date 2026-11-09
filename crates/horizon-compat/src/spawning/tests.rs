use super::*;
use crate::host::{BlockFacts, MemoryHost};

const PENGUIN: &str = "test/Penguin";
const ANIMAL_INIT: &str = "net/minecraft/world/entity/animal/Animal.<init>.(Lnet/minecraft/world/entity/EntityType;Lnet/minecraft/world/level/Level;)V";
const FACTORY_CREATE: &str = "create.(Lnet/minecraft/world/entity/EntityType;Lnet/minecraft/world/level/Level;)Lnet/minecraft/world/entity/Entity;";
const TEST: &str = "test.(Lnet/minecraft/world/entity/EntityType;Lnet/minecraft/world/level/ServerLevelAccessor;Lnet/minecraft/world/entity/MobSpawnType;Lnet/minecraft/core/BlockPos;Lnet/minecraft/util/RandomSource;)Z";
const MODIFY: &str = "modify.(Lnet/minecraft/core/Holder;Lnet/neoforged/neoforge/common/world/BiomeModifier$Phase;Lnet/neoforged/neoforge/common/world/ModifiableBiomeInfo$BiomeInfo$Builder;)V";
const REGISTER: &str = "net/neoforged/neoforge/event/entity/RegisterSpawnPlacementsEvent.register.(Lnet/minecraft/world/entity/EntityType;Lnet/minecraft/world/entity/SpawnPlacementType;Lnet/minecraft/world/level/levelgen/Heightmap$Types;Lnet/minecraft/world/entity/SpawnPlacements$SpawnPredicate;Lnet/neoforged/neoforge/event/entity/RegisterSpawnPlacementsEvent$Operation;)V";
const REGISTER_OR: &str = "net/neoforged/neoforge/event/entity/RegisterSpawnPlacementsEvent.register.(Lnet/minecraft/world/entity/EntityType;Lnet/minecraft/world/entity/SpawnPlacements$SpawnPredicate;)V";
const REGISTER_BY: &str = "net/neoforged/neoforge/event/entity/RegisterSpawnPlacementsEvent.register.(Lnet/minecraft/world/entity/EntityType;Lnet/minecraft/world/entity/SpawnPlacements$SpawnPredicate;Lnet/neoforged/neoforge/event/entity/RegisterSpawnPlacementsEvent$Operation;)V";
const CHECK_RULES: &str = "net/minecraft/world/entity/SpawnPlacements.checkSpawnRules.(Lnet/minecraft/world/entity/EntityType;Lnet/minecraft/world/level/ServerLevelAccessor;Lnet/minecraft/world/entity/MobSpawnType;Lnet/minecraft/core/BlockPos;Lnet/minecraft/util/RandomSource;)Z";
const GET_PLACEMENT: &str = "net/minecraft/world/entity/SpawnPlacements.getPlacementType.(Lnet/minecraft/world/entity/EntityType;)Lnet/minecraft/world/entity/SpawnPlacementType;";
const ANIMAL_RULES: &str = "net/minecraft/world/entity/animal/Animal.checkAnimalSpawnRules.(Lnet/minecraft/world/entity/EntityType;Lnet/minecraft/world/level/LevelAccessor;Lnet/minecraft/world/entity/MobSpawnType;Lnet/minecraft/core/BlockPos;Lnet/minecraft/util/RandomSource;)Z";
const ANY_LIGHT_MONSTER: &str = "net/minecraft/world/entity/monster/Monster.checkAnyLightMonsterSpawnRules.(Lnet/minecraft/world/entity/EntityType;Lnet/minecraft/world/level/LevelAccessor;Lnet/minecraft/world/entity/MobSpawnType;Lnet/minecraft/core/BlockPos;Lnet/minecraft/util/RandomSource;)Z";
const BRIGHT: &str = "net/minecraft/world/entity/animal/Animal.isBrightEnoughToSpawn.(Lnet/minecraft/world/level/BlockAndTintGetter;Lnet/minecraft/core/BlockPos;)Z";
const MOB_SPAWN_RULES: &str = "net/minecraft/world/entity/Mob.checkSpawnRules.(Lnet/minecraft/world/level/LevelAccessor;Lnet/minecraft/world/entity/MobSpawnType;)Z";
const IS_BABY: &str = "net/minecraft/world/entity/LivingEntity.isBaby.()Z";
const GROUP_BY_CHANCE: &str = "net/minecraft/world/entity/AgeableMob$AgeableMobGroupData.<init>.(F)V";
const GROUP_GROWS: &str = "net/minecraft/world/entity/AgeableMob$AgeableMobGroupData.increaseGroupSizeByOne.()V";
const GET_SPAWN_SETTINGS: (&str, &str) = (
    "net/neoforged/neoforge/common/world/ModifiableBiomeInfo$BiomeInfo$Builder.getMobSpawnSettings.()Lnet/neoforged/neoforge/common/world/MobSpawnSettingsBuilder;",
    "getMobSpawnSettings.()Lnet/neoforged/neoforge/common/world/MobSpawnSettingsBuilder;",
);
const ADD_SPAWN: &str = "net/neoforged/neoforge/common/world/MobSpawnSettingsBuilder.addSpawn.(Lnet/minecraft/world/entity/MobCategory;Lnet/minecraft/world/level/biome/MobSpawnSettings$SpawnerData;)Lnet/minecraft/world/level/biome/MobSpawnSettings$Builder;";
const ADD_CHARGE: &str = "net/neoforged/neoforge/common/world/MobSpawnSettingsBuilder.addMobCharge.(Lnet/minecraft/world/entity/EntityType;DD)Lnet/minecraft/world/level/biome/MobSpawnSettings$Builder;";
const REMOVE_COST: &str = "net/neoforged/neoforge/common/world/MobSpawnSettingsBuilder.removeSpawnCost.(Lnet/minecraft/world/entity/EntityType;)Lnet/neoforged/neoforge/common/world/MobSpawnSettingsBuilder;";
const ENTITY_TYPE_BY_ID: &str = "net/minecraft/world/entity/EntityType.byString.(Ljava/lang/String;)Ljava/util/Optional;";

/// A mod as Waddles writes one: its penguins' factory; a spawn rule that always says yes, one
/// that always says no; and a biome modifier that, in the add phase, gives each biome its spawner
/// (under its type's category) and a spawn cost, and takes the cows' cost away.
fn vcall(rt: &mut Rt, owner_key: &'static str, key: &'static str, args: &[V]) -> V {
    let class = rt.class_of(args[0]);
    match key {
        FACTORY_CREATE if class == "test/PenguinFactory" => {
            let e = rt.new_object(PENGUIN);
            api::call(rt, ANIMAL_INIT, &[e, args[1], args[2]]);
            e
        }
        TEST if class == "test/Yes" => V::I(1),
        TEST if class == "test/No" => V::I(0),
        MODIFY if class == "test/PenguinModifier" => {
            if rt.str_of(rt.get_field(args[2], "$name")) != Some("ADD") {
                return V::Void;
            }
            let settings = api::virtual_call(rt, GET_SPAWN_SETTINGS, &[args[3]]);
            let spawner = rt.get_field(args[0], "spawn");
            let ty = rt.get_field(spawner, "type");
            let category = api::call(rt, GET_CATEGORY, &[ty]);
            api::call(rt, ADD_SPAWN, &[settings, category, spawner]);
            api::call(rt, ADD_CHARGE, &[settings, ty, V::D(0.7), V::D(0.15)]);
            let cow_id = rt.statics["t.cow"];
            let cow = api::call(rt, ENTITY_TYPE_BY_ID, &[cow_id]);
            let cow = rt.get_field(cow, "value");
            api::call(rt, REMOVE_COST, &[settings, cow]);
            V::Void
        }
        _ => api::call(rt, owner_key, args),
    }
}

/// A runtime with the mod's penguin class (an animal with no code of its own) and its type, a
/// creature 0.6 by 1 block registered as `test:penguin`.
fn runtime() -> (Rt, V) {
    let mut rt = Rt::new();
    crate::install();
    rt.vcall = Some(vcall);
    rt.supers.insert(PENGUIN, ANIMAL);
    let ty = rt.new_object("net/minecraft/world/entity/EntityType");
    rt.put_field(ty, "$width", V::F(0.6));
    rt.put_field(ty, "$height", V::F(1.0));
    let factory = rt.new_object("test/PenguinFactory");
    rt.put_field(ty, "$factory", factory);
    let creature = crate::api_common::plain_enum(&mut rt, MOB_CATEGORY, "CREATURE").unwrap();
    rt.put_field(ty, "$category", creature);
    rt.registered.insert(("entity_type".into(), "test:penguin".into()), ty);
    (rt, ty)
}

fn sturdy() -> BlockFacts {
    BlockFacts { hardness: 0.6, push: 0, replaceable: false, sturdy: (1 << 18) - 1 }
}

/// A server's world: a grass floor at y 63, 17 by 17 blocks, grass a block animals spawn on.
fn floor() -> MemoryHost {
    let mut host = MemoryHost::default();
    for x in -8..=8 {
        for z in -8..=8 {
            host.blocks.insert([x, 63, z], "minecraft:grass_block[snowy=false]".into());
        }
    }
    host.facts.insert("minecraft:grass_block".into(), sturdy());
    host.facts.insert("minecraft:stone".into(), sturdy());
    host.block_tags.insert("minecraft:animals_spawnable_on".into(), vec!["minecraft:grass_block".into()]);
    host
}

fn statics(rt: &mut Rt) -> (V, V, V, V) {
    let ground = api::get_static(rt, "net/minecraft/world/entity/SpawnPlacementTypes.ON_GROUND");
    let heightmap = api::get_static(rt, "net/minecraft/world/level/levelgen/Heightmap$Types.MOTION_BLOCKING_NO_LEAVES");
    let and = api::get_static(rt, "net/neoforged/neoforge/event/entity/RegisterSpawnPlacementsEvent$Operation.AND");
    let replace = api::get_static(rt, "net/neoforged/neoforge/event/entity/RegisterSpawnPlacementsEvent$Operation.REPLACE");
    (ground, heightmap, and, replace)
}

/// `SpawnPlacements.checkSpawnRules` of `ty` at the origin for a natural spawn.
fn rules(rt: &mut Rt, ty: V) -> bool {
    let level = crate::glue::level(rt);
    let natural = spawn_type(rt, "NATURAL");
    let p = api::block_pos(rt, 0, 64, 0);
    api::call(rt, CHECK_RULES, &[ty, level, natural, p, V::NULL]).i() != 0
}

/// The class of what the last call threw, taking it.
fn thrown(rt: &mut Rt) -> Option<&'static str> {
    rt.thrown.take().map(|t| rt.class_of(t))
}

/// NeoForge's registrations, merged: a type's first needs its placement and heightmap, whatever
/// its operation; later ones add an `OR` (the two-argument form's) or an `AND`, and a `REPLACE`
/// swaps the first predicate (the `OR`s and `AND`s stay). Only a `REPLACE` may change the
/// placement or heightmap. A type nobody registered spawns anywhere, with no restrictions.
#[test]
fn registrations_merge_as_neoforges_merged_predicate() {
    let (mut rt, ty) = runtime();
    let (ground, heightmap, and, replace) = statics(&mut rt);
    let ev = rt.new_object(REGISTER_EVENT);
    let (yes, no) = (rt.new_object("test/Yes"), rt.new_object("test/No"));
    assert!(rules(&mut rt, ty), "no registration: no rule");
    let unrestricted = api::call(&mut rt, GET_PLACEMENT, &[ty]);
    assert_eq!(rt.str_of(rt.get_field(unrestricted, "$placement")), Some("no_restrictions"));
    api::call(&mut rt, REGISTER, &[ev, ty, V::NULL, heightmap, yes, and]);
    assert_eq!(thrown(&mut rt), Some("java/lang/NullPointerException"));
    assert_eq!(spawn_types(&mut rt)[0].placement, None, "a failed first registration leaves none");
    api::call(&mut rt, REGISTER, &[ev, ty, ground, heightmap, no, and]);
    assert_eq!(thrown(&mut rt), None);
    assert!(!rules(&mut rt, ty), "the first one's rule, its AND ignored");
    api::call(&mut rt, REGISTER_OR, &[ev, ty, yes]);
    assert!(rules(&mut rt, ty), "no, or yes");
    api::call(&mut rt, REGISTER_BY, &[ev, ty, no, and]);
    assert!(!rules(&mut rt, ty), "(no or yes) and no");
    api::call(&mut rt, REGISTER, &[ev, ty, V::NULL, V::NULL, yes, replace]);
    assert!(!rules(&mut rt, ty), "the replacement takes the first's place: (yes or yes) and no");
    api::call(&mut rt, REGISTER, &[ev, ty, ground, heightmap, yes, and]);
    assert_eq!(thrown(&mut rt), Some("java/lang/IllegalStateException"));
    assert_eq!(api::call(&mut rt, GET_PLACEMENT, &[ty]), ground);
    let types = spawn_types(&mut rt);
    assert_eq!(types.len(), 1);
    let penguin = &types[0];
    assert_eq!((penguin.id.as_str(), penguin.category.as_str()), ("test:penguin", "creature"));
    assert_eq!((penguin.placement.as_deref(), penguin.heightmap.as_str()), (Some("on_ground"), "MOTION_BLOCKING_NO_LEAVES"));
    assert!(penguin.far_from_player, "a creature may spawn far from players");
    assert_eq!((penguin.width, penguin.height, penguin.spawn_scale), (0.6, 1.0, 1.0));
}

/// `SpawnerData.CODEC` reads vanilla's fields and refuses what vanilla does: counts below 1 or out
/// of order, a negative weight, a type no registry holds.
#[test]
fn a_spawner_reads_vanillas_fields_and_refuses_what_vanilla_does() {
    let (mut rt, ty) = runtime();
    let ok = serde_json::json!({"type": "test:penguin", "weight": 10, "minCount": 1, "maxCount": 4});
    let d = crate::recipes::decode_native(&mut rt, "spawner_data", &ok).unwrap();
    assert_eq!(rt.get_field(d, "type"), ty);
    assert_eq!((rt.get_field(d, "minCount"), rt.get_field(d, "maxCount")), (V::I(1), V::I(4)));
    assert_eq!(rt.get_field(rt.get_field(d, "weight"), "value"), V::I(10));
    let bad = |rt: &mut Rt, j: serde_json::Value| crate::recipes::decode_native(rt, "spawner_data", &j).unwrap_err();
    assert_eq!(
        bad(&mut rt, serde_json::json!({"type": "test:penguin", "weight": 10, "minCount": 4, "maxCount": 1})),
        "minCount needs to be smaller or equal to maxCount"
    );
    assert!(bad(&mut rt, serde_json::json!({"type": "test:penguin", "weight": -1, "minCount": 1, "maxCount": 1})).contains("Weight"));
    assert!(bad(&mut rt, serde_json::json!({"type": "test:penguin", "weight": 1, "minCount": 0, "maxCount": 1})).contains("positive"));
    assert!(bad(&mut rt, serde_json::json!({"type": "test:walrus", "weight": 1, "minCount": 1, "maxCount": 1})).contains("test:walrus"));
}

/// A mod's code biome modifier edits the biome's spawns through NeoForge's builder: the spawner it
/// decoded goes into its type's category after what the biome had, its spawn cost is added, and the
/// cost it removes (the cows') is gone. Another mod's spawner (a type this runtime doesn't hold) is kept as it
/// was, as is the creature probability.
#[test]
fn a_code_biome_modifier_adds_its_spawner_and_cost_through_the_builder() {
    let (mut rt, _) = runtime();
    let cow_id = rt.string("minecraft:cow");
    rt.statics.insert("t.cow", cow_id);
    let modifier = rt.new_object("test/PenguinModifier");
    let json = serde_json::json!({"type": "test:penguin", "weight": 10, "minCount": 1, "maxCount": 4});
    let spawner = crate::recipes::decode_native(&mut rt, "spawner_data", &json).unwrap();
    rt.put_field(modifier, "spawn", spawner);
    let yeti = Spawner { entity: "other:yeti".into(), weight: 2, min: 1, max: 1 };
    let cow = Spawner { entity: "minecraft:cow".into(), weight: 8, min: 4, max: 4 };
    let mut spawns = BiomeSpawns {
        categories: BTreeMap::from([("creature".to_owned(), vec![cow.clone(), yeti.clone()])]),
        costs: BTreeMap::from([("other:yeti".to_owned(), (0.12, 0.9)), ("minecraft:cow".to_owned(), (0.1, 0.5))]),
        probability: 0.07,
    };
    let mut host = MemoryHost::default();
    rt.with_host(&mut host, |rt| {
        for phase in 0..crate::worldgen::PHASES.len() {
            crate::worldgen::modify_biome(rt, modifier, "minecraft:snowy_plains", phase, &mut Vec::new(), &mut spawns).unwrap();
        }
    });
    let penguin = Spawner { entity: "test:penguin".into(), weight: 10, min: 1, max: 4 };
    assert_eq!(spawns.categories, BTreeMap::from([("creature".to_owned(), vec![cow, yeti, penguin])]));
    assert_eq!(spawns.costs, BTreeMap::from([("other:yeti".to_owned(), (0.12, 0.9)), ("test:penguin".to_owned(), (0.15, 0.7))]));
    assert_eq!(spawns.probability, 0.07);
}

/// Vanilla's spawn rules as a mod registers or calls them read the world: an animal's needs a
/// block below that animals spawn on and light above 8 (a trial spawner's needs no light); a
/// monster's of any light needs a block below a mob may stand on, but for a spawner's.
#[test]
fn vanillas_spawn_rules_read_the_block_below_and_the_light() {
    let (mut rt, ty) = runtime();
    let mut host = floor();
    host.blocks.insert([3, 63, 3], "minecraft:stone".into());
    host.light.insert([1, 64, 1], (0, 8));
    let called = |rt: &mut Rt, key: &'static str, kind: &str, at: [i64; 3]| {
        let level = crate::glue::level(rt);
        let kind = spawn_type(rt, kind);
        let p = api::block_pos(rt, at[0], at[1], at[2]);
        api::call(rt, key, &[ty, level, kind, p, V::NULL]).i() != 0
    };
    rt.with_host(&mut host, |rt| {
        assert!(called(rt, ANIMAL_RULES, "NATURAL", [0, 64, 0]), "grass in daylight");
        assert!(!called(rt, ANIMAL_RULES, "NATURAL", [1, 64, 1]), "grass, light 8");
        assert!(called(rt, ANIMAL_RULES, "TRIAL_SPAWNER", [1, 64, 1]), "a trial spawner's needs no light");
        assert!(!called(rt, ANIMAL_RULES, "NATURAL", [3, 64, 3]), "stone isn't a block animals spawn on");
        assert!(called(rt, ANY_LIGHT_MONSTER, "NATURAL", [3, 64, 3]), "stone holds a mob");
        assert!(!called(rt, ANY_LIGHT_MONSTER, "NATURAL", [12, 64, 12]), "air below");
        assert!(called(rt, ANY_LIGHT_MONSTER, "SPAWNER", [12, 64, 12]), "a spawner's needs no floor");
        let level = crate::glue::level(rt);
        let (lit, dim) = (api::block_pos(rt, 0, 64, 0), api::block_pos(rt, 1, 64, 1));
        assert_eq!(api::call(rt, BRIGHT, &[level, lit]), V::I(1));
        assert_eq!(api::call(rt, BRIGHT, &[level, dim]), V::I(0));
    });
}

fn penguin(rt: &mut Rt, ty: V, at: [f64; 3]) -> V {
    let level = crate::glue::level(rt);
    let e = api::call(rt, CREATE_IN, &[ty, level]);
    crate::mod_entities::set_pos(rt, e, at);
    e
}

/// An animal's own spawn check (`PathfinderMob.checkSpawnRules`): grass below is always good; on
/// stone it wants light, the sky's darkened by the time of day as vanilla reads it, so not at
/// midnight.
#[test]
fn an_animal_spawns_on_stone_by_day_and_on_grass_by_night() {
    let (mut rt, ty) = runtime();
    let mut host = floor();
    host.blocks.insert([3, 63, 3], "minecraft:stone".into());
    let check = |rt: &mut Rt, e: V| {
        let level = crate::glue::level(rt);
        let natural = spawn_type(rt, "NATURAL");
        api::call(rt, MOB_SPAWN_RULES, &[e, level, natural]).i() != 0
    };
    rt.with_host(&mut host, |rt| {
        let on_grass = penguin(rt, ty, [0.5, 64.0, 0.5]);
        let on_stone = penguin(rt, ty, [3.5, 64.0, 3.5]);
        assert!(check(rt, on_grass) && check(rt, on_stone), "noon");
    });
    host.day_time = 18000;
    rt.with_host(&mut host, |rt| {
        let on_grass = penguin(rt, ty, [0.5, 64.0, 0.5]);
        let on_stone = penguin(rt, ty, [3.5, 64.0, 3.5]);
        assert!(check(rt, on_grass), "grass at midnight");
        assert!(!check(rt, on_stone), "stone at midnight");
    });
}

/// A pack of ageable mobs shares one group (`AgeableMobGroupData`): its first mob starts it (babies
/// by a chance of 0.05, never the first) and each one counts; a group sure of babies makes the
/// next one a baby.
#[test]
fn a_pack_of_ageable_mobs_shares_its_group() {
    let (mut rt, ty) = runtime();
    let mut host = floor();
    rt.with_host(&mut host, |rt| {
        let level = crate::glue::level(rt);
        let natural = spawn_type(rt, "NATURAL");
        let first = penguin(rt, ty, [0.5, 64.0, 0.5]);
        let group = crate::entity_saves::finalize_mob_in_group(rt, first, level, natural, V::NULL);
        assert_eq!(rt.class_of(group), AGEABLE_GROUP);
        assert_eq!(rt.get_field(group, "groupSize"), V::I(1));
        assert_eq!(rt.get_field(group, "babySpawnChance"), V::F(0.05));
        assert_eq!(api::call(rt, IS_BABY, &[first]), V::I(0));
        let sure = rt.new_object(AGEABLE_GROUP);
        api::call(rt, GROUP_BY_CHANCE, &[sure, V::F(1.0)]);
        api::call(rt, GROUP_GROWS, &[sure]);
        let second = penguin(rt, ty, [2.5, 64.0, 2.5]);
        assert_eq!(crate::entity_saves::finalize_mob_in_group(rt, second, level, natural, sure), sure);
        assert_eq!(rt.get_field(sure, "groupSize"), V::I(2));
        assert_eq!(api::call(rt, IS_BABY, &[second]), V::I(1));
    });
}

/// The game's spawner asks for a mod's mob: it is made, placed facing the yaw given, checked and
/// added, its box and cluster size told; the spawn caps then count it. Another where it stands, or
/// where a mob the pass spawned stands, or in water, is refused; a type no registry holds makes
/// none.
#[test]
fn the_spawner_makes_a_mods_mob_where_the_world_lets_it() {
    let (mut rt, ty) = runtime();
    let (ground, heightmap, and, _) = statics(&mut rt);
    let ev = rt.new_object(REGISTER_EVENT);
    let yes = rt.new_object("test/Yes");
    api::call(&mut rt, REGISTER, &[ev, ty, ground, heightmap, yes, and]);
    let mut host = floor();
    host.blocks.insert([5, 64, 5], "minecraft:water[level=0]".into());
    let request = |at: [f64; 3], first: bool, pending: &'static [[f64; 6]]| SpawnRequest {
        entity: "test:penguin",
        position: at,
        yaw: 90.0,
        reason: "NATURAL",
        first,
        group_size: 1,
        distance: Some(100.0),
        pending,
    };
    rt.with_host(&mut host, |rt| {
        assert!(spawn_rules(rt, "test:penguin", "NATURAL", [0, 64, 0]));
        let SpawnOutcome::Joined { bounds, max_cluster, group_full } = spawn_mob(rt, &request([0.5, 64.0, 0.5], true, &[])) else {
            panic!("refused")
        };
        let expected = [0.2, 64.0, 0.2, 0.8, 65.0, 0.8];
        assert!(bounds.iter().zip(expected).all(|(a, b)| (a - b).abs() < 1e-6), "{bounds:?}");
        assert_eq!((max_cluster, group_full), (4, false));
        let live = crate::mod_entities::live(rt);
        assert_eq!(live.len(), 1);
        assert_eq!(crate::mod_entities::xyz(rt, live[0]), [0.5, 64.0, 0.5]);
        assert_eq!(rt.get_field(live[0], "$yRot"), V::F(90.0));
        let counted = census(rt, "minecraft:overworld");
        let [one] = &counted[..] else { panic!("{counted:?}") };
        let blocks = one.blocks.expect("a living mob blocks building");
        assert!(blocks.iter().zip(expected).all(|(a, b)| (a - b).abs() < 1e-6), "{blocks:?}");
        let expected =
            Census { kind: "test:penguin".into(), category: "creature".into(), block: [0, 64, 0], mob: true, persistent: false, blocks: one.blocks };
        assert_eq!(counted, vec![expected]);
        assert_eq!(spawn_mob(rt, &request([0.9, 64.0, 0.9], false, &[])), SpawnOutcome::Refused, "the first stands there");
        static PENDING_BOX: [[f64; 6]; 1] = [[2.0, 64.0, 2.0, 3.0, 66.0, 3.0]];
        assert_eq!(spawn_mob(rt, &request([2.5, 64.0, 2.5], false, &PENDING_BOX)), SpawnOutcome::Refused, "a pending mob stands there");
        assert_eq!(spawn_mob(rt, &request([5.5, 64.0, 5.5], false, &[])), SpawnOutcome::Refused, "in water");
        let unknown = SpawnRequest { entity: "test:walrus", ..request([0.5, 64.0, 6.5], false, &[]) };
        assert_eq!(spawn_mob(rt, &unknown), SpawnOutcome::Uncreated);
        assert_eq!(crate::mod_entities::live(rt).len(), 1);
        // Chunk generation's region holds no entities: the first one is in nobody's way there.
        let generated = SpawnRequest { reason: "CHUNK_GENERATION", distance: None, ..request([0.9, 64.0, 0.9], true, &[]) };
        assert!(matches!(spawn_mob(rt, &generated), SpawnOutcome::Joined { .. }));
        assert_eq!(crate::mod_entities::live(rt).len(), 2);
    });
}
