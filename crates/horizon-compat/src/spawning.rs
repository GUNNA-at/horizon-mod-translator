//! Natural spawning of the mods' mobs, as NeoForge 1.21.1 runs it: where each entity type may
//! spawn (`RegisterSpawnPlacementsEvent`, `SpawnPlacements`), the vanilla spawn predicates a mod
//! registers or calls (`Animal.checkAnimalSpawnRules`, `Monster.isDarkEnoughToSpawn`...), the
//! spawners a biome modifier adds (`MobSpawnSettings.SpawnerData`, NeoForge's
//! `MobSpawnSettingsBuilder`), and the game's questions about the mods' types as its spawner runs:
//! what each type is ([`spawn_types`]), whether a place passes its rules ([`spawn_rules`]), the
//! mob it makes there, checked, finalized and added ([`spawn_mob`]), and the mods' entities the
//! spawn caps count ([`census`]).
//!
//! A registration lands on the entity type object (`$spawn*` fields), merged as NeoForge's
//! `MergedSpawnPredicate` merges them: the first one's placement, heightmap and predicate, then
//! `OR`s, `AND`s, and `REPLACE`s of the first predicate (and of the placement or heightmap). Not modelled: NeoForge's `SpawnPlacementCheck`,
//! `PositionCheck` and `FinalizeSpawn` events (their default answers are taken), and a mod's
//! registration over a vanilla type, which the game spawns by its own rules.
use crate::{Data, Rt, Table, V, api};
use std::collections::BTreeMap;

const PLACEMENT_TYPES: &str = "net/minecraft/world/entity/SpawnPlacementTypes";
const PLACEMENT_TYPE: &str = "net/minecraft/world/entity/SpawnPlacementType";
const SPAWN_PLACEMENTS: &str = "net/minecraft/world/entity/SpawnPlacements";
const REGISTER_EVENT: &str = "net/neoforged/neoforge/event/entity/RegisterSpawnPlacementsEvent";
const SPAWNER_DATA: &str = "net/minecraft/world/level/biome/MobSpawnSettings$SpawnerData";
const SETTINGS_BUILDER: &str = "net/neoforged/neoforge/common/world/MobSpawnSettingsBuilder";
const VANILLA_SETTINGS_BUILDER: &str = "net/minecraft/world/level/biome/MobSpawnSettings$Builder";
const SPAWN_COST: &str = "net/minecraft/world/level/biome/MobSpawnSettings$MobSpawnCost";
const MOB_SPAWN_TYPE: &str = "net/minecraft/world/entity/MobSpawnType";
const MOB_CATEGORY: &str = "net/minecraft/world/entity/MobCategory";
const MOB: &str = "net/minecraft/world/entity/Mob";
const PATHFINDER: &str = "net/minecraft/world/entity/PathfinderMob";
const ANIMAL: &str = "net/minecraft/world/entity/animal/Animal";
const MONSTER: &str = "net/minecraft/world/entity/monster/Monster";
const WATER_ANIMAL: &str = "net/minecraft/world/entity/animal/WaterAnimal";
/// `AgeableMob.AgeableMobGroupData`: the group data a pack of ageable mobs shares.
pub(crate) const AGEABLE_GROUP: &str = "net/minecraft/world/entity/AgeableMob$AgeableMobGroupData";
const PREDICATE_TEST: &str = "net/minecraft/world/entity/SpawnPlacements$SpawnPredicate.test.(Lnet/minecraft/world/entity/EntityType;Lnet/minecraft/world/level/ServerLevelAccessor;Lnet/minecraft/world/entity/MobSpawnType;Lnet/minecraft/core/BlockPos;Lnet/minecraft/util/RandomSource;)Z";
const POSITION_OK: (&str, &str) = (
    "net/minecraft/world/entity/SpawnPlacementType.isSpawnPositionOk.(Lnet/minecraft/world/level/LevelReader;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/entity/EntityType;)Z",
    "isSpawnPositionOk.(Lnet/minecraft/world/level/LevelReader;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/entity/EntityType;)Z",
);
const CHECK_SPAWN_RULES: (&str, &str) = (
    "net/minecraft/world/entity/Mob.checkSpawnRules.(Lnet/minecraft/world/level/LevelAccessor;Lnet/minecraft/world/entity/MobSpawnType;)Z",
    "checkSpawnRules.(Lnet/minecraft/world/level/LevelAccessor;Lnet/minecraft/world/entity/MobSpawnType;)Z",
);
const CHECK_OBSTRUCTION: (&str, &str) = (
    "net/minecraft/world/entity/Mob.checkSpawnObstruction.(Lnet/minecraft/world/level/LevelReader;)Z",
    "checkSpawnObstruction.(Lnet/minecraft/world/level/LevelReader;)Z",
);
const REMOVE_FAR: (&str, &str) = ("net/minecraft/world/entity/Mob.removeWhenFarAway.(D)Z", "removeWhenFarAway.(D)Z");
const MAX_CLUSTER: (&str, &str) = ("net/minecraft/world/entity/Mob.getMaxSpawnClusterSize.()I", "getMaxSpawnClusterSize.()I");
const GROUP_FULL: (&str, &str) = ("net/minecraft/world/entity/Mob.isMaxGroupSizeReached.(I)Z", "isMaxGroupSizeReached.(I)Z");
const IS_PERSISTENT: (&str, &str) = ("net/minecraft/world/entity/Mob.isPersistenceRequired.()Z", "isPersistenceRequired.()Z");
const CUSTOM_PERSISTENCE: (&str, &str) = ("net/minecraft/world/entity/Mob.requiresCustomPersistence.()Z", "requiresCustomPersistence.()Z");
const MOVE_TO: (&str, &str) = ("net/minecraft/world/entity/Entity.moveTo.(DDDFF)V", "moveTo.(DDDFF)V");
const CREATE_IN: &str = "net/minecraft/world/entity/EntityType.create.(Lnet/minecraft/world/level/Level;)Lnet/minecraft/world/entity/Entity;";
const GET_CATEGORY: &str = "net/minecraft/world/entity/EntityType.getCategory.()Lnet/minecraft/world/entity/MobCategory;";
const DESPAWN_DISTANCE: &str = "net/minecraft/world/entity/MobCategory.getDespawnDistance.()I";
const LEVEL_RANDOM: (&str, &str) = (
    "net/minecraft/world/level/Level.getRandom.()Lnet/minecraft/util/RandomSource;",
    "getRandom.()Lnet/minecraft/util/RandomSource;",
);
const NEXT_INT_BOUND: &str = "net/minecraft/util/RandomSource.nextInt.(I)I";
const STATE_FLUID: &str = "net/minecraft/world/level/block/state/BlockState.getFluidState.()Lnet/minecraft/world/level/material/FluidState;";
const FLUID_EMPTY: &str = "net/minecraft/world/level/material/FluidState.isEmpty.()Z";
/// The group data the pack being spawned shares (`SpawnGroupData`), from its first mob's
/// `finalizeSpawn` on.
const GROUP: &str = "compat:spawn-group";
/// The boxes of the mobs the spawn's pass made that the level doesn't hold yet, while one is
/// checked.
const PENDING: &str = "compat:spawn-pending";
/// Set while chunk generation checks a mob: its region holds no entities
/// (`WorldGenRegion.getEntities`), so none stands in its way.
const REGION: &str = "compat:spawn-region";
/// `MobCategory`'s constants by ordinal, as their serialized names.
pub const CATEGORIES: [&str; 8] =
    ["monster", "creature", "ambient", "axolotls", "underground_water_creature", "water_creature", "water_ambient", "misc"];
/// `SpawnPlacementTypes`' constants, by the names the game reads them by.
const PLACEMENTS: [(&str, &str); 4] =
    [("ON_GROUND", "on_ground"), ("IN_WATER", "in_water"), ("IN_LAVA", "in_lava"), ("NO_RESTRICTIONS", "no_restrictions")];
/// `isFaceSturdy(UP)` with full support, in `BlockFacts::sturdy`.
const UP_FULL: u32 = 1 << 1;

/// One spawner of a biome's mob spawn settings (`MobSpawnSettings.SpawnerData`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Spawner {
    pub entity: String,
    pub weight: u32,
    pub min: u32,
    pub max: u32,
}

/// A biome's mob spawn settings as NeoForge's `MobSpawnSettingsBuilder` holds them while biome
/// modifiers edit them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BiomeSpawns {
    /// The spawners by category, by its serialized name (`creature`).
    pub categories: BTreeMap<String, Vec<Spawner>>,
    /// The spawn costs by entity type: its energy budget and charge.
    pub costs: BTreeMap<String, (f64, f64)>,
    /// `creatureGenerationProbability`.
    pub probability: f32,
}

/// A mod's entity type as the game's natural spawning reads it.
#[derive(Clone, Debug, PartialEq)]
pub struct SpawnType {
    pub id: String,
    /// Its category's serialized name (`creature`).
    pub category: String,
    pub width: f32,
    pub height: f32,
    /// `EntityType.Builder.spawnDimensionsScale`: how much bigger its spawn's box is.
    pub spawn_scale: f32,
    pub fire_immune: bool,
    /// `canSpawnFarFromPlayer`.
    pub far_from_player: bool,
    /// `canSummon`.
    pub summonable: bool,
    /// Its placement type (`on_ground`, `in_water`, `in_lava`, `no_restrictions`), `custom` for a
    /// mod's own (which [`spawn_rules`] asks), None where no mod registered one
    /// (`NO_RESTRICTIONS`).
    pub placement: Option<String>,
    /// `SpawnPlacements.getHeightmapType` (`MOTION_BLOCKING_NO_LEAVES`...).
    pub heightmap: String,
}

/// A mob the game's spawner asks a mod to make (`getMobForSpawn` and what follows it).
#[derive(Clone, Debug, PartialEq)]
pub struct SpawnRequest<'a> {
    pub entity: &'a str,
    pub position: [f64; 3],
    pub yaw: f32,
    /// The `MobSpawnType` constant (`NATURAL`, `CHUNK_GENERATION`).
    pub reason: &'a str,
    /// Its pack's first mob: the pack's group data starts empty.
    pub first: bool,
    /// The pack's size with it (`isMaxGroupSizeReached`).
    pub group_size: u32,
    /// The nearest player's squared distance (natural spawning's `isValidPositionForMob`); None
    /// where none is asked (chunk generation).
    pub distance: Option<f64>,
    /// The boxes of the mobs this pass spawned that the level doesn't hold yet (the game's own,
    /// added once the pass ends): they stand in a new mob's way as the level's do.
    pub pending: &'a [[f64; 6]],
}

/// What came of a [`SpawnRequest`].
#[derive(Clone, Debug, PartialEq)]
pub enum SpawnOutcome {
    /// The type made no mob: natural spawning ends its pass there (`getMobForSpawn` gave null),
    /// chunk generation goes on to its next try without moving.
    Uncreated,
    /// The mob refused the place (too far, its rules, an obstruction): none joined.
    Refused,
    /// It was finalized and added: its box, `getMaxSpawnClusterSize`, and whether its pack is full
    /// (`isMaxGroupSizeReached`).
    Joined { bounds: [f64; 6], max_cluster: u32, group_full: bool },
}

/// A mod's entity as the spawn caps count it (`NaturalSpawner.createState`), and as it stands in
/// the way of the game's own spawns (`isUnobstructed`).
#[derive(Clone, Debug, PartialEq)]
pub struct Census {
    pub kind: String,
    /// Its category's serialized name.
    pub category: String,
    /// `blockPosition()`.
    pub block: [i32; 3],
    pub mob: bool,
    /// `isPersistenceRequired() || requiresCustomPersistence()`.
    pub persistent: bool,
    /// Its box when it blocks building (`blocksBuilding`: a living one, a boat...).
    pub blocks: Option<[f64; 6]>,
}

/// `SpawnPlacementTypes.<name>`: one object each, knowing its name.
fn placement_type(rt: &mut Rt, i: usize) -> V {
    let name = PLACEMENTS[i].1;
    rt.intern(format!("compat:spawn-placement/{name}"), move |rt| {
        let p = rt.new_object(PLACEMENT_TYPE);
        let n = rt.string(name);
        rt.put_field(p, "$placement", n);
        p
    })
}

/// The statics answered here: `SpawnPlacementTypes`' constants.
pub(crate) fn get_static(rt: &mut Rt, owner: &str, field: &str) -> Option<V> {
    if owner != PLACEMENT_TYPES {
        return None;
    }
    let i = PLACEMENTS.iter().position(|(f, _)| *f == field)?;
    Some(placement_type(rt, i))
}

/// Fires `RegisterSpawnPlacementsEvent` once, after the entity types have their attributes
/// (`SpawnPlacements.fireSpawnPlacementEvent`, from `GameData.postRegisterEvents`).
pub(crate) fn fire(rt: &mut Rt) {
    const FIRED: &str = "compat:spawn-placements-fired";
    if rt.interned.contains_key(FIRED) {
        return;
    }
    rt.interned.insert(FIRED.into(), V::I(1));
    let ev = rt.new_object(REGISTER_EVENT);
    crate::neo::post(rt, REGISTER_EVENT, ev);
}

/// Entity type `id` as the registry holds it: a mod's, or vanilla's.
fn entity_type(rt: &mut Rt, id: &str) -> Option<V> {
    let id = if id.contains(':') { id.to_owned() } else { format!("minecraft:{id}") };
    crate::entity_saves::type_named(rt, &id)
}

/// Whether type `ty` has a registration: vanilla registers its own types.
fn registered(rt: &Rt, ty: V) -> bool {
    rt.get_field(ty, "$spawnRegistered").i() != 0
        || crate::mod_entities::type_id(rt, ty).is_some_and(crate::vanilla_entities::is_vanilla_type)
}

/// `RegisterSpawnPlacementsEvent.register(type, placement, heightmap, predicate, operation)`: a
/// type's first registration needs its placement and heightmap (its operation doesn't matter);
/// later ones merge their predicate by `operation`, and only `REPLACE` may change the placement or
/// heightmap.
fn register(rt: &mut Rt, ty: V, placement: V, heightmap: V, predicate: V, operation: V) {
    let id = crate::mod_entities::type_id(rt, ty).unwrap_or("?").to_owned();
    if !registered(rt, ty) {
        if placement.is_null() || heightmap.is_null() {
            let what = if placement.is_null() { "Spawn placement type" } else { "Heightmap type" };
            crate::jlang::throw_with(rt, "java/lang/NullPointerException", &format!("{what} must be specified for the first registration of {id}"));
            return;
        }
        rt.put_field(ty, "$spawnRegistered", V::I(1));
        rt.put_field(ty, "$spawnPlacement", placement);
        rt.put_field(ty, "$spawnHeightmap", heightmap);
        rt.put_field(ty, "$spawnPredicate", predicate);
        return;
    }
    let op = rt.str_of(rt.get_field(operation, "$name")).unwrap_or("OR").to_owned();
    if op != "REPLACE" && (!placement.is_null() || !heightmap.is_null()) {
        crate::jlang::throw_with(rt, "java/lang/IllegalStateException", &format!("Only REPLACE may change the spawn placement or heightmap of {id}"));
        return;
    }
    match op.as_str() {
        "AND" | "OR" => {
            let field = if op == "AND" { "$spawnAnd" } else { "$spawnOr" };
            let list = match rt.get_field(ty, field) {
                l if !l.is_null() => l,
                _ => {
                    let l = crate::jutil::new_list(rt, Vec::new());
                    rt.put_field(ty, field, l);
                    l
                }
            };
            crate::jutil::list_add(rt, list, predicate);
        }
        _ => {
            rt.put_field(ty, "$spawnRegistered", V::I(1));
            rt.put_field(ty, "$spawnReplace", predicate);
            if !placement.is_null() {
                rt.put_field(ty, "$spawnPlacement", placement);
            }
            if !heightmap.is_null() {
                rt.put_field(ty, "$spawnHeightmap", heightmap);
            }
        }
    }
}

/// One registered predicate's `test`, an exception it throws reported as a refusal.
fn test(rt: &mut Rt, predicate: V, args: &[V]) -> bool {
    let r = rt.call_functional(PREDICATE_TEST, predicate, args);
    if rt.thrown.is_some() {
        rt.report_thrown("SpawnPredicate.test");
        return false;
    }
    r.i() != 0
}

/// `SpawnPlacements.checkSpawnRules(type, level, spawnType, pos, random)`: the merged predicate
/// (`MergedSpawnPredicate.build`: the first one, or the last `REPLACE`'s in its stead, or any `OR`;
/// then every `AND`); true for a type with none.
fn check_spawn_rules(rt: &mut Rt, ty: V, level: V, kind: V, pos: V, random: V) -> bool {
    let args = [ty, level, kind, pos, random];
    let base = match rt.get_field(ty, "$spawnReplace") {
        r if !r.is_null() => r,
        _ => rt.get_field(ty, "$spawnPredicate"),
    };
    let mut ok = base.is_null() || test(rt, base, &args);
    if !ok {
        for p in crate::jutil::elements(rt, rt.get_field(ty, "$spawnOr")) {
            if test(rt, p, &args) {
                ok = true;
                break;
            }
        }
    }
    ok && crate::jutil::elements(rt, rt.get_field(ty, "$spawnAnd")).into_iter().all(|p| test(rt, p, &args))
}

/// The block state at `p` as the host's world has it.
fn state_text(rt: &Rt, p: [i32; 3]) -> String {
    rt.host().map_or_else(|| "minecraft:air".to_owned(), |h| h.block_state(p))
}

fn block_of(text: &str) -> &str {
    crate::host::split_state(text).0
}

/// `MobSpawnType.ignoresLightRequirements`: a trial spawner's.
fn ignores_light(rt: &Rt, kind: V) -> bool {
    rt.str_of(rt.get_field(kind, "$name")) == Some("TRIAL_SPAWNER")
}

/// `MobSpawnType.isSpawner`.
fn is_spawner(rt: &Rt, kind: V) -> bool {
    matches!(rt.str_of(rt.get_field(kind, "$name")), Some("SPAWNER" | "TRIAL_SPAWNER"))
}

/// `BlockState.isValidSpawn(level, pos, type)` of the block at `p` for type `ty`: the game's
/// answer, else a face sturdy on top (`isFaceSturdy(UP)`, full support), magma only for a
/// fire-immune type.
fn valid_spawn_on(rt: &Rt, ty: V, p: [i32; 3]) -> bool {
    let fire_immune = rt.get_field(ty, "$fireImmune").i() != 0;
    let Some(h) = rt.host() else { return false };
    if let Some(valid) = h.valid_spawn(p, fire_immune) {
        return valid;
    }
    let text = h.block_state(p);
    if block_of(&text) == "minecraft:magma_block" {
        return fire_immune;
    }
    h.block_facts(&text).is_some_and(|f| f.sturdy & UP_FULL != 0)
}

/// `Mob.checkMobSpawnRules`: a spawner's, or on a block a mob may spawn on.
fn mob_rules(rt: &Rt, ty: V, kind: V, p: [i32; 3]) -> bool {
    is_spawner(rt, kind) || valid_spawn_on(rt, ty, [p[0], p[1] - 1, p[2]])
}

/// `Animal.isBrightEnoughToSpawn`: raw brightness above 8.
fn bright_enough(rt: &Rt, p: [i32; 3]) -> bool {
    crate::level_env::raw_brightness(rt, p, 0) > 8
}

/// `Monster.isDarkEnoughToSpawn(level, pos, random)`, by 26.3's dimension types: no more sky light
/// than a draw under 32; the overworld's and the End's block light 0 (the Nether's any); then the
/// brightness (darkened as for a thunderstorm when one rages) no more than the dimension's test, a
/// draw of 0 to 7 in the overworld, 7 in the Nether, 15 in the End.
fn dark_enough(rt: &mut Rt, p: [i32; 3], random: V) -> bool {
    let (block, sky) = rt.host().map_or((0, 15), |h| h.light(p));
    if i32::from(sky) > api::call(rt, NEXT_INT_BOUND, &[random, V::I(32)]).i() {
        return false;
    }
    let dimension = rt.host().map(|h| h.dimension()).unwrap_or_default();
    let nether = dimension == "minecraft:the_nether";
    if !nether && block > 0 {
        return false;
    }
    let thundering = !nether && rt.host().is_some_and(|h| f64::from(h.weather().1) > 0.9);
    let darken = if thundering { 10 } else { crate::level_env::sky_darken(rt) };
    let brightness = crate::level_env::raw_brightness(rt, p, darken);
    let test = match dimension.as_str() {
        "minecraft:the_nether" => 7,
        "minecraft:the_end" => 15,
        _ => api::call(rt, NEXT_INT_BOUND, &[random, V::I(8)]).i(),
    };
    brightness <= test
}

/// Whether a block state holds a fluid (`getFluidState().isEmpty()` false).
fn has_fluid(rt: &mut Rt, text: &str) -> bool {
    let s = crate::api_common::state(rt, text);
    let fluid = api::call(rt, STATE_FLUID, &[s]);
    rt.thrown = None;
    !fluid.is_null() && api::call(rt, FLUID_EMPTY, &[fluid]).i() == 0
}

/// `WaterAnimal.checkSurfaceWaterAnimalSpawnRules`: within 13 below the sea's level, a fluid below
/// and water above.
fn surface_water(rt: &mut Rt, p: [i32; 3]) -> bool {
    const SEA_LEVEL: i32 = 63;
    if p[1] < SEA_LEVEL - 13 || p[1] > SEA_LEVEL {
        return false;
    }
    let below = state_text(rt, [p[0], p[1] - 1, p[2]]);
    let above = state_text(rt, [p[0], p[1] + 1, p[2]]);
    has_fluid(rt, &below) && block_of(&above) == "minecraft:water"
}

/// `Level.containsAnyLiquid(box)`.
fn any_liquid(rt: &mut Rt, b: [f64; 6]) -> bool {
    let lo = [b[0].floor() as i32, b[1].floor() as i32, b[2].floor() as i32];
    let hi = [b[3].ceil() as i32, b[4].ceil() as i32, b[5].ceil() as i32];
    for x in lo[0]..hi[0] {
        for y in lo[1]..hi[1] {
            for z in lo[2]..hi[2] {
                let text = state_text(rt, [x, y, z]);
                if has_fluid(rt, &text) {
                    return true;
                }
            }
        }
    }
    false
}

/// Whether two boxes overlap (`AABB.intersects`).
fn meets(a: [f64; 6], b: [f64; 6]) -> bool {
    a[0] < b[3] && a[3] > b[0] && a[1] < b[4] && a[4] > b[1] && a[2] < b[5] && a[5] > b[2]
}

/// Mod entity `e`'s box where it stands.
fn bounds(rt: &mut Rt, e: V) -> [f64; 6] {
    let [x, y, z] = crate::mod_entities::xyz(rt, e);
    let [w, h, _] = crate::mod_entities::own_size(rt, e);
    let half = f64::from(w) / 2.0;
    [x - half, y, z - half, x + half, y + f64::from(h), z + half]
}

/// Whether mod entity `e` is in level `dimension`, not removed.
fn in_level(rt: &Rt, e: V, dimension: &str) -> bool {
    rt.get_field(e, "removalReason").is_null() && rt.str_of(rt.get_field(e, "$dimension")).is_none_or(|d| d == dimension)
}

/// Whether one of the game's entities stands in the way of building (`Entity.blocksBuilding`): a
/// living one, a boat, a minecart, lit TNT, a falling block, an end crystal.
fn game_entity_blocks(kind: &str, living: bool) -> bool {
    living
        || kind.ends_with("_boat")
        || kind.ends_with("_raft")
        || kind.contains("minecart")
        || matches!(kind, "minecraft:tnt" | "minecraft:falling_block" | "minecraft:end_crystal")
}

/// A game entity's box where the host says it stands, its type's size as vanilla's table holds it.
fn view_box(v: &crate::host::EntityView) -> [f64; 6] {
    let (w, h, _) = crate::vanilla_entities::dimensions(&v.kind);
    let half = f64::from(w) / 2.0;
    [v.pos[0] - half, v.pos[1], v.pos[2] - half, v.pos[0] + half, v.pos[1] + f64::from(h), v.pos[2] + half]
}

/// `Level.isUnobstructed(entity)`: no other entity that blocks building (the game's, the players,
/// the mods', the pass's pending mobs) overlaps its box.
fn unobstructed(rt: &mut Rt, e: V) -> bool {
    if rt.interned.contains_key(REGION) {
        return true;
    }
    let b = bounds(rt, e);
    let pending = rt.interned.get(PENDING).map(|&l| crate::jutil::elements(rt, l)).unwrap_or_default();
    for p in pending {
        let c: Vec<f64> = crate::jutil::elements(rt, p).into_iter().map(V::d).collect();
        if let [x0, y0, z0, x1, y1, z1] = c[..]
            && meets([x0, y0, z0, x1, y1, z1], b)
        {
            return false;
        }
    }
    let (game, players, dimension) = rt
        .host()
        .map_or((Vec::new(), Vec::new(), String::new()), |h| (h.entities_in(b), h.player_entities(), h.dimension()));
    if game.iter().any(|v| game_entity_blocks(&v.kind, v.health.is_some()) && meets(view_box(v), b)) {
        return false;
    }
    let player_box = |p: [f64; 3]| [p[0] - 0.3, p[1], p[2] - 0.3, p[0] + 0.3, p[1] + 1.8, p[2] + 0.3];
    if players.iter().any(|(_, _, p)| meets(player_box(*p), b)) {
        return false;
    }
    for other in crate::mod_entities::live(rt) {
        if other == e || !in_level(rt, other, &dimension) {
            continue;
        }
        if blocks_building(rt, other) && meets(bounds(rt, other), b) {
            return false;
        }
    }
    true
}

/// `Entity.blocksBuilding` of a mod's entity: a living one, or one that set it.
fn blocks_building(rt: &mut Rt, e: V) -> bool {
    rt.get_field(e, "blocksBuilding").i() != 0 || crate::mobs::is_mod_living(rt, e)
}

/// `Mob.checkSpawnRules(level, spawnType)` as vanilla's classes answer it for a mod's mob: a
/// pathfinder's walk target value where it stands may not be negative (an animal's grass, else the
/// light; a monster's dark; its own where it says); any other mob may spawn.
pub(crate) fn mob_check_spawn_rules(rt: &mut Rt, e: V) -> bool {
    if !rt.instance_of(e, PATHFINDER) {
        return true;
    }
    let p = crate::mobs::block_position(rt, e);
    crate::mobs::walk_target_value(rt, e, p) >= 0.0
}

/// `Mob.checkSpawnObstruction(level)`: no liquid in its box, and nothing in its way.
pub(crate) fn mob_check_obstruction(rt: &mut Rt, e: V) -> bool {
    let b = bounds(rt, e);
    !any_liquid(rt, b) && unobstructed(rt, e)
}

/// `new AgeableMob.AgeableMobGroupData(shouldSpawnBaby, babySpawnChance)`.
pub(crate) fn ageable_group_data(rt: &mut Rt, baby: bool, chance: f32) -> V {
    let g = rt.new_object(AGEABLE_GROUP);
    rt.put_field(g, "shouldSpawnBaby", V::I(i32::from(baby)));
    rt.put_field(g, "babySpawnChance", V::F(chance));
    rt.put_field(g, "groupSize", V::I(0));
    g
}

/// The `MobSpawnType` constant named.
fn spawn_type(rt: &mut Rt, name: &str) -> V {
    let key = match name {
        "CHUNK_GENERATION" => "net/minecraft/world/entity/MobSpawnType.CHUNK_GENERATION",
        "SPAWNER" => "net/minecraft/world/entity/MobSpawnType.SPAWNER",
        "STRUCTURE" => "net/minecraft/world/entity/MobSpawnType.STRUCTURE",
        "EVENT" => "net/minecraft/world/entity/MobSpawnType.EVENT",
        "PATROL" => "net/minecraft/world/entity/MobSpawnType.PATROL",
        "TRIAL_SPAWNER" => "net/minecraft/world/entity/MobSpawnType.TRIAL_SPAWNER",
        _ => "net/minecraft/world/entity/MobSpawnType.NATURAL",
    };
    api::get_static(rt, key)
}

/// The serialized name of entity type `ty`'s category (`EntityType.getCategory`).
fn category_of_type(rt: &mut Rt, ty: V) -> String {
    let c = api::call(rt, GET_CATEGORY, &[ty]);
    rt.thrown = None;
    if c.is_null() {
        return "misc".into();
    }
    let i = crate::api_common::enum_ordinal(rt, c).clamp(0, 7) as usize;
    CATEGORIES[i].to_owned()
}

/// The category of entity type `id` (a mod's registered one, or vanilla's), by its serialized
/// name; None for a type the registry doesn't hold.
pub fn category_of(rt: &mut Rt, id: &str) -> Option<String> {
    let ty = entity_type(rt, id)?;
    Some(category_of_type(rt, ty))
}

/// The mod's registered entity types with their categories' serialized names, as the content
/// root's spawn modifiers file them.
pub fn entity_categories(rt: &mut Rt) -> Vec<(String, String)> {
    let mut types: Vec<(String, V)> =
        rt.registered.iter().filter(|((registry, _), _)| registry == "entity_type").map(|((_, id), &ty)| (id.clone(), ty)).collect();
    types.sort_by(|a, b| a.0.cmp(&b.0));
    types.into_iter().map(|(id, ty)| (id, category_of_type(rt, ty))).collect()
}

/// The mods' entity types natural spawning may make, as the game reads them.
pub fn spawn_types(rt: &mut Rt) -> Vec<SpawnType> {
    let mut out = Vec::new();
    for (id, summonable) in crate::entity_saves::mod_types(rt) {
        let Some(&ty) = rt.registered.get(&("entity_type".to_owned(), id.clone())) else { continue };
        let category = category_of_type(rt, ty);
        let (width, height, _) = crate::mod_entities::type_size(rt, ty);
        let far = rt.get_field(ty, "$canSpawnFar").i() != 0 || matches!(category.as_str(), "creature" | "misc");
        let placement = rt.get_field(ty, "$spawnPlacement");
        let placement = (!placement.is_null()).then(|| match rt.str_of(rt.get_field(placement, "$placement")) {
            Some(name) => name.to_owned(),
            None => "custom".to_owned(),
        });
        let heightmap = rt.get_field(ty, "$spawnHeightmap");
        let heightmap = rt.str_of(rt.get_field(heightmap, "$name")).unwrap_or("MOTION_BLOCKING_NO_LEAVES").to_owned();
        out.push(SpawnType {
            id,
            category,
            width,
            height,
            spawn_scale: crate::entity_saves::spawn_scale(rt, ty),
            fire_immune: rt.get_field(ty, "$fireImmune").i() != 0,
            far_from_player: far,
            summonable,
            placement,
            heightmap,
        });
    }
    out
}

/// Whether mod type `id` may spawn at `pos` for `reason` (a `MobSpawnType` constant's name): its
/// own placement type's answer where it registered one of its own (`isSpawnPositionOk`; the game
/// answers vanilla's), then `SpawnPlacements.checkSpawnRules`. True for a type nobody registered.
pub fn spawn_rules(rt: &mut Rt, id: &str, reason: &str, pos: [i32; 3]) -> bool {
    let Some(ty) = entity_type(rt, id) else { return true };
    let level = crate::glue::level(rt);
    let p = api::block_pos(rt, pos[0].into(), pos[1].into(), pos[2].into());
    let placement = rt.get_field(ty, "$spawnPlacement");
    if !placement.is_null() && rt.get_field(placement, "$placement").is_null() {
        let ok = api::virtual_call(rt, POSITION_OK, &[placement, level, p, ty]);
        if rt.thrown.is_some() {
            rt.report_thrown("SpawnPlacementType.isSpawnPositionOk");
            return false;
        }
        if ok.i() == 0 {
            return false;
        }
    }
    let kind = spawn_type(rt, reason);
    let random = api::virtual_call(rt, LEVEL_RANDOM, &[level]);
    rt.thrown = None;
    check_spawn_rules(rt, ty, level, kind, p, random)
}

/// A virtual call's truth on mob `e`, an exception reported as `default`.
fn ask(rt: &mut Rt, e: V, key: (&'static str, &'static str), args: &[V], default: bool) -> bool {
    let mut all = vec![e];
    all.extend_from_slice(args);
    let v = api::virtual_call(rt, key, &all);
    if rt.thrown.is_some() {
        rt.report_thrown(key.1);
        return default;
    }
    v.i() != 0
}

/// The game's spawner makes a mob of mod type `id` (`getMobForSpawn`; `moveTo` with its yaw),
/// checks it where it stands (natural spawning's `isValidPositionForMob`: not past its category's
/// despawn distance when it would despawn there; then NeoForge's position check, its
/// `checkSpawnRules` and `checkSpawnObstruction`), finalizes it with its pack's group data and adds
/// it with its passengers.
pub fn spawn_mob(rt: &mut Rt, request: &SpawnRequest<'_>) -> SpawnOutcome {
    let natural = request.distance.is_some();
    let Some(ty) = entity_type(rt, request.entity) else { return SpawnOutcome::Uncreated };
    if request.first {
        rt.interned.insert(GROUP.into(), V::NULL);
    }
    let level = crate::glue::level(rt);
    let e = api::call(rt, CREATE_IN, &[ty, level]);
    if rt.thrown.is_some() {
        rt.report_thrown("EntityType.create");
        return SpawnOutcome::Uncreated;
    }
    if e.is_null() {
        return SpawnOutcome::Uncreated;
    }
    if !crate::mobs::is_mob(rt, e) {
        // Natural spawning makes only mobs (`getMobForSpawn`); chunk generation made it, and goes on.
        return if natural { SpawnOutcome::Uncreated } else { SpawnOutcome::Refused };
    }
    let [x, y, z] = request.position;
    api::virtual_call(rt, MOVE_TO, &[e, V::D(x), V::D(y), V::D(z), V::F(request.yaw), V::F(0.0)]);
    rt.report_thrown("moveTo");
    if let Some(d) = request.distance {
        let c = api::call(rt, GET_CATEGORY, &[ty]);
        rt.thrown = None;
        let far = if c.is_null() { 128 } else { api::call(rt, DESPAWN_DISTANCE, &[c]).i() };
        if d > f64::from(far * far) && ask(rt, e, REMOVE_FAR, &[V::D(d)], false) {
            return SpawnOutcome::Refused;
        }
    }
    let kind = spawn_type(rt, request.reason);
    let pending: Vec<V> = request.pending.iter().map(|b| crate::jutil::new_list(rt, b.iter().map(|&c| V::D(c)).collect())).collect();
    let pending = crate::jutil::new_list(rt, pending);
    rt.interned.insert(PENDING.into(), pending);
    if !natural {
        rt.interned.insert(REGION.into(), V::I(1));
    }
    let placed = ask(rt, e, CHECK_SPAWN_RULES, &[level, kind], false) && ask(rt, e, CHECK_OBSTRUCTION, &[level], false);
    rt.interned.remove(PENDING);
    rt.interned.remove(REGION);
    if !placed {
        return SpawnOutcome::Refused;
    }
    let group = rt.interned.get(GROUP).copied().unwrap_or(V::NULL);
    let group = crate::entity_saves::finalize_mob_in_group(rt, e, level, kind, group);
    rt.interned.insert(GROUP.into(), group);
    let tree = crate::mod_entities::self_and_passengers(rt, e);
    crate::entity_saves::add_tree(rt, level, &tree, false);
    let max_cluster = api::virtual_call(rt, MAX_CLUSTER, &[e]);
    rt.report_thrown("getMaxSpawnClusterSize");
    let group_full = ask(rt, e, GROUP_FULL, &[V::I(request.group_size as i32)], false);
    SpawnOutcome::Joined { bounds: bounds(rt, e), max_cluster: max_cluster.i().max(0) as u32, group_full }
}

/// The mods' entities in level `dimension` as the spawn caps count them (the game leaves out the
/// `misc` category's): their type, category, block, whether a mob that must stay, and their box
/// where they block building.
pub fn census(rt: &mut Rt, dimension: &str) -> Vec<Census> {
    let mut out = Vec::new();
    for e in crate::mod_entities::live(rt) {
        if !in_level(rt, e, dimension) {
            continue;
        }
        let ty = rt.get_field(e, "$type");
        let Some(kind) = crate::mod_entities::type_id(rt, ty).map(str::to_owned) else { continue };
        let category = category_of_type(rt, ty);
        let mob = crate::mobs::is_mob(rt, e);
        let persistent = mob && (ask(rt, e, IS_PERSISTENT, &[], false) || ask(rt, e, CUSTOM_PERSISTENCE, &[], false));
        let block = crate::mod_entities::xyz(rt, e).map(|c| c.floor() as i32);
        let blocks = blocks_building(rt, e).then(|| bounds(rt, e));
        out.push(Census { kind, category, block, mob, persistent, blocks });
    }
    out
}

// ---- spawners and a biome's spawn settings ----

/// `Weight.of(w)`: the data-class table's record (its `value`).
fn weight(rt: &mut Rt, w: i32) -> V {
    crate::records::make_data(rt, "net/minecraft/util/random/Weight", "(I)V", &[V::I(w)])
}

/// `new SpawnerData(type, weight, minCount, maxCount)`: its fields under vanilla's names (mods
/// read `type`, `minCount` and `maxCount` directly), its weight a `Weight`.
fn init_spawner(rt: &mut Rt, d: V, ty: V, w: i32, min: i32, max: i32) {
    rt.put_field(d, "type", ty);
    let w = weight(rt, w);
    rt.put_field(d, "weight", w);
    rt.put_field(d, "minCount", V::I(min));
    rt.put_field(d, "maxCount", V::I(max));
}

fn spawner_data(rt: &mut Rt, ty: V, w: i32, min: i32, max: i32) -> V {
    let d = rt.new_object(SPAWNER_DATA);
    init_spawner(rt, d, ty, w, min, max);
    d
}

/// Entity type `id` for a biome's spawn settings: the registry's, else one that only knows its id
/// (another mod's, which this runtime doesn't hold, kept as it was).
fn settings_type(rt: &mut Rt, id: &str) -> V {
    entity_type(rt, id).unwrap_or_else(|| crate::mod_entities::vanilla_type(rt, id))
}

/// A spawner object's entry, None where its type has no id.
fn spawner_of(rt: &Rt, d: V) -> Option<Spawner> {
    let entity = crate::mod_entities::type_id(rt, rt.get_field(d, "type"))?.to_owned();
    let w = rt.get_field(d, "weight");
    Some(Spawner {
        entity,
        weight: rt.get_field(w, "value").i().max(0) as u32,
        min: rt.get_field(d, "minCount").i().max(0) as u32,
        max: rt.get_field(d, "maxCount").i().max(0) as u32,
    })
}

/// `MobSpawnSettings.SpawnerData.CODEC`: `{type, weight, minCount, maxCount}`, the counts at least
/// 1 and in order, the weight not negative, the type a registered one.
pub fn decode_spawner(rt: &mut Rt, j: &serde_json::Value) -> Result<V, String> {
    let o = j.as_object().ok_or_else(|| format!("Not a map: {j}"))?;
    let int = |k: &str| o.get(k).and_then(serde_json::Value::as_i64).ok_or_else(|| format!("No key {k} in MapLike[{j}]"));
    let id = o.get("type").and_then(serde_json::Value::as_str).ok_or_else(|| format!("No key type in MapLike[{j}]"))?;
    let ty = entity_type(rt, id).ok_or_else(|| format!("Unknown registry key in ResourceKey[minecraft:root / minecraft:entity_type]: {id}"))?;
    let (w, min, max) = (int("weight")?, int("minCount")?, int("maxCount")?);
    if w < 0 {
        return Err(format!("Weight should be >= 0: {w}"));
    }
    for v in [min, max] {
        if v < 1 {
            return Err(format!("Value must be positive: {v}"));
        }
    }
    if min > max {
        return Err("minCount needs to be smaller or equal to maxCount".into());
    }
    Ok(spawner_data(rt, ty, w as i32, min as i32, max as i32))
}

/// The builder a biome modifier edits (`MobSpawnSettingsBuilder` of the biome's settings): a list
/// per category in `MobCategory` order, the spawn costs by type, the creature probability.
pub(crate) fn settings_builder(rt: &mut Rt, spawns: &BiomeSpawns) -> V {
    let b = rt.new_object(SETTINGS_BUILDER);
    let mut lists = Vec::with_capacity(CATEGORIES.len());
    for name in CATEGORIES {
        let mut items = Vec::new();
        for s in spawns.categories.get(name).into_iter().flatten() {
            let ty = settings_type(rt, &s.entity);
            items.push(spawner_data(rt, ty, s.weight as i32, s.min as i32, s.max as i32));
        }
        lists.push(crate::jutil::new_list(rt, items));
    }
    let lists = crate::jutil::new_list(rt, lists);
    rt.put_field(b, "$lists", lists);
    let costs = rt.alloc("java/util/HashMap", Data::Table(Table::default()));
    for (id, &(budget, charge)) in &spawns.costs {
        let ty = settings_type(rt, id);
        let cost = crate::records::make(rt, SPAWN_COST, &[V::D(budget), V::D(charge)]);
        crate::jutil::table_put(rt, costs, ty, cost);
    }
    rt.put_field(b, "$costs", costs);
    rt.put_field(b, "$probability", V::F(spawns.probability));
    b
}

/// What builder `b` holds after the modifiers ran.
pub(crate) fn read_settings(rt: &mut Rt, b: V) -> BiomeSpawns {
    let mut out = BiomeSpawns { probability: rt.get_field(b, "$probability").f(), ..BiomeSpawns::default() };
    for (i, list) in crate::jutil::elements(rt, rt.get_field(b, "$lists")).into_iter().enumerate() {
        let Some(name) = CATEGORIES.get(i) else { break };
        let entries: Vec<Spawner> = crate::jutil::elements(rt, list).into_iter().filter_map(|d| spawner_of(rt, d)).collect();
        if !entries.is_empty() {
            out.categories.insert((*name).to_owned(), entries);
        }
    }
    for (ty, cost) in crate::jutil::table_pairs(rt, rt.get_field(b, "$costs")) {
        let Some(id) = crate::mod_entities::type_id(rt, ty).map(str::to_owned) else { continue };
        out.costs.insert(id, (rt.get_field(cost, "energyBudget").d(), rt.get_field(cost, "charge").d()));
    }
    out
}

/// Builder `b`'s list for a `MobCategory` constant.
fn category_list(rt: &Rt, b: V, category: V) -> V {
    let i = crate::api_common::enum_ordinal(rt, category).clamp(0, 7) as usize;
    crate::jutil::elements(rt, rt.get_field(b, "$lists")).get(i).copied().unwrap_or(V::NULL)
}

// ---- calls ----

/// The keys [`call`] may answer, by owner, name and descriptor alone.
pub(crate) fn claims(owner: &str, name: &str, desc: &str) -> bool {
    matches!(owner, REGISTER_EVENT | SPAWN_PLACEMENTS | SPAWNER_DATA | SETTINGS_BUILDER | VANILLA_SETTINGS_BUILDER)
        || (owner == MOB_SPAWN_TYPE && matches!(name, "isSpawner" | "ignoresLightRequirements"))
        || (owner == PLACEMENT_TYPE && name == "isSpawnPositionOk")
        || (owner == MOB && name == "checkMobSpawnRules")
        || (owner == ANIMAL && matches!(name, "checkAnimalSpawnRules" | "isBrightEnoughToSpawn"))
        || (owner == MONSTER && matches!(name, "checkMonsterSpawnRules" | "checkAnyLightMonsterSpawnRules" | "isDarkEnoughToSpawn"))
        || (owner == WATER_ANIMAL && name == "checkSurfaceWaterAnimalSpawnRules")
        || (owner == AGEABLE_GROUP
            && (matches!(name, "increaseGroupSizeByOne" | "getGroupSize" | "isShouldSpawnBaby" | "getBabySpawnChance")
                || (name == "<init>" && matches!(desc, "(ZF)V" | "(Z)V" | "(F)V"))))
}

pub(crate) fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, name, desc) = crate::parts(key)?;
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let truth = |b: bool| V::I(i32::from(b));
    Some(match (owner, name) {
        (REGISTER_EVENT, "register") => {
            match desc {
                // `register(type, predicate)`: an `OR`; `register(type, predicate, operation)`.
                "(Lnet/minecraft/world/entity/EntityType;Lnet/minecraft/world/entity/SpawnPlacements$SpawnPredicate;)V" => {
                    let or = api::get_static(rt, "net/neoforged/neoforge/event/entity/RegisterSpawnPlacementsEvent$Operation.OR");
                    register(rt, a(1), V::NULL, V::NULL, a(2), or);
                }
                "(Lnet/minecraft/world/entity/EntityType;Lnet/minecraft/world/entity/SpawnPlacements$SpawnPredicate;Lnet/neoforged/neoforge/event/entity/RegisterSpawnPlacementsEvent$Operation;)V" => {
                    register(rt, a(1), V::NULL, V::NULL, a(2), a(3));
                }
                _ => register(rt, a(1), a(2), a(3), a(4), a(5)),
            }
            V::Void
        }
        (SPAWN_PLACEMENTS, "checkSpawnRules") => {
            let ok = !registered(rt, a(0)) || check_spawn_rules(rt, a(0), a(1), a(2), a(3), a(4));
            truth(ok)
        }
        (SPAWN_PLACEMENTS, "getPlacementType") => match rt.get_field(a(0), "$spawnPlacement") {
            p if !p.is_null() => p,
            _ => placement_type(rt, 3),
        },
        (SPAWN_PLACEMENTS, "getHeightmapType") => match rt.get_field(a(0), "$spawnHeightmap") {
            h if !h.is_null() => h,
            _ => api::get_static(rt, "net/minecraft/world/level/levelgen/Heightmap$Types.MOTION_BLOCKING_NO_LEAVES"),
        },
        (SPAWN_PLACEMENTS, "isSpawnPositionOk") => {
            let placement = match rt.get_field(a(0), "$spawnPlacement") {
                p if !p.is_null() => p,
                _ => placement_type(rt, 3),
            };
            let ok = api::virtual_call(rt, POSITION_OK, &[placement, a(1), a(2), a(0)]);
            truth(ok.i() != 0)
        }
        // Vanilla's placement types, which a mod's code may ask itself: no restrictions; in a
        // fluid; on the ground (a block below a mob may spawn on, room above).
        (PLACEMENT_TYPE, "isSpawnPositionOk") => {
            let which = rt.str_of(rt.get_field(a(0), "$placement")).unwrap_or("no_restrictions").to_owned();
            let p = api::pos_of(rt, a(2));
            let at = state_text(rt, p);
            truth(match which.as_str() {
                "in_water" => block_of(&at) == "minecraft:water" || at.contains("waterlogged=true"),
                "in_lava" => block_of(&at) == "minecraft:lava",
                "on_ground" => {
                    let above = state_text(rt, [p[0], p[1] + 1, p[2]]);
                    let empty = |rt: &mut Rt, s: &str| {
                        let full = rt.host().and_then(|h| h.block_facts(s)).is_some_and(|f| f.sturdy & UP_FULL != 0);
                        !full && !has_fluid(rt, s)
                    };
                    valid_spawn_on(rt, a(3), [p[0], p[1] - 1, p[2]]) && empty(rt, &at) && empty(rt, &above)
                }
                _ => true,
            })
        }
        (MOB_SPAWN_TYPE, "isSpawner") => truth(is_spawner(rt, a(0))),
        (MOB_SPAWN_TYPE, "ignoresLightRequirements") => truth(ignores_light(rt, a(0))),
        // The vanilla spawn predicates (type, level, spawn type, position, random).
        (MOB, "checkMobSpawnRules") => {
            let p = api::pos_of(rt, a(3));
            truth(mob_rules(rt, a(0), a(2), p))
        }
        (ANIMAL, "isBrightEnoughToSpawn") => {
            let p = api::pos_of(rt, a(1));
            truth(bright_enough(rt, p))
        }
        (ANIMAL, "checkAnimalSpawnRules") => {
            let p = api::pos_of(rt, a(3));
            let below = state_text(rt, [p[0], p[1] - 1, p[2]]);
            let on = rt.host().is_some_and(|h| h.block_in_tag(block_of(&below), "minecraft:animals_spawnable_on"));
            truth(on && (ignores_light(rt, a(2)) || bright_enough(rt, p)))
        }
        (MONSTER, "isDarkEnoughToSpawn") => {
            let p = api::pos_of(rt, a(1));
            truth(dark_enough(rt, p, a(2)))
        }
        // Not peaceful: the game spawns no monsters then, and asks none.
        (MONSTER, "checkMonsterSpawnRules") => {
            let p = api::pos_of(rt, a(3));
            let ok = (ignores_light(rt, a(2)) || dark_enough(rt, p, a(4))) && mob_rules(rt, a(0), a(2), p);
            truth(ok)
        }
        (MONSTER, "checkAnyLightMonsterSpawnRules") => {
            let p = api::pos_of(rt, a(3));
            truth(mob_rules(rt, a(0), a(2), p))
        }
        (WATER_ANIMAL, "checkSurfaceWaterAnimalSpawnRules") => {
            let p = api::pos_of(rt, a(3));
            truth(surface_water(rt, p))
        }
        // `new AgeableMobGroupData(shouldSpawnBaby, babySpawnChance)`, `(shouldSpawnBaby)` (a chance
        // of 0.05), `(babySpawnChance)` (a baby).
        (AGEABLE_GROUP, "<init>") => {
            let (baby, chance) = match desc {
                "(ZF)V" => (a(1).i() != 0, a(2).f()),
                "(F)V" => (true, a(1).f()),
                _ => (a(1).i() != 0, 0.05),
            };
            rt.put_field(a(0), "shouldSpawnBaby", V::I(i32::from(baby)));
            rt.put_field(a(0), "babySpawnChance", V::F(chance));
            rt.put_field(a(0), "groupSize", V::I(0));
            V::Void
        }
        (AGEABLE_GROUP, "increaseGroupSizeByOne") => {
            let n = rt.get_field(a(0), "groupSize").i();
            rt.put_field(a(0), "groupSize", V::I(n + 1));
            V::Void
        }
        (AGEABLE_GROUP, "getGroupSize") => V::I(rt.get_field(a(0), "groupSize").i()),
        (AGEABLE_GROUP, "isShouldSpawnBaby") => V::I(rt.get_field(a(0), "shouldSpawnBaby").i()),
        (AGEABLE_GROUP, "getBabySpawnChance") => V::F(rt.get_field(a(0), "babySpawnChance").f()),
        (SPAWNER_DATA, "<init>") => {
            let w = if desc.starts_with("(Lnet/minecraft/world/entity/EntityType;Lnet/minecraft/util/random/Weight;") {
                rt.get_field(a(2), "value").i()
            } else {
                a(2).i()
            };
            init_spawner(rt, a(0), a(1), w, a(3).i(), a(4).i());
            V::Void
        }
        (SPAWNER_DATA, "getWeight") => rt.get_field(a(0), "weight"),
        (SPAWNER_DATA, "toString") => {
            let s = spawner_of(rt, a(0)).unwrap_or_default();
            rt.string(&format!("{}*({}-{}):{}", s.entity, s.min, s.max, s.weight))
        }
        (SETTINGS_BUILDER | VANILLA_SETTINGS_BUILDER, _) if !rt.get_field(a(0), "$lists").is_null() => match name {
            "addSpawn" => {
                let list = category_list(rt, a(0), a(1));
                crate::jutil::list_add(rt, list, a(2));
                a(0)
            }
            "getSpawner" => category_list(rt, a(0), a(1)),
            "getSpawnerTypes" => {
                let all: Vec<V> =
                    CATEGORIES.iter().filter_map(|c| crate::api_common::plain_enum(rt, MOB_CATEGORY, &c.to_ascii_uppercase())).collect();
                crate::jutil::new_set_of(rt, &all)
            }
            "addMobCharge" => {
                // `addMobCharge(type, charge, energyBudget)`: `new MobSpawnCost(energyBudget, charge)`.
                let cost = crate::records::make(rt, SPAWN_COST, &[a(3), a(2)]);
                let costs = rt.get_field(a(0), "$costs");
                crate::jutil::table_put(rt, costs, a(1), cost);
                a(0)
            }
            "getCost" => {
                let costs = rt.get_field(a(0), "$costs");
                crate::jutil::table_get(rt, costs, a(1)).unwrap_or(V::NULL)
            }
            "removeSpawnCost" => {
                let costs = rt.get_field(a(0), "$costs");
                crate::jutil::table_remove(rt, costs, a(1));
                a(0)
            }
            "getEntityTypes" => {
                let keys = crate::jutil::table_keys(rt, rt.get_field(a(0), "$costs"));
                crate::jutil::new_set_of(rt, &keys)
            }
            "creatureGenerationProbability" => {
                rt.put_field(a(0), "$probability", a(1));
                a(0)
            }
            "getProbability" => rt.get_field(a(0), "$probability"),
            _ => return None,
        },
        _ => return None,
    })
}

#[cfg(test)]
mod tests;
