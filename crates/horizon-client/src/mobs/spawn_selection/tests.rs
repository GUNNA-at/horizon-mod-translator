//! The spawner's path for a translated mod's entity types against its path for the game's own
//! (which tools/test_natural_spawn_parity.py holds to vanilla's): a mod's type whose mod answers
//! as vanilla's cow does makes the same draws, tests and spawns as the cow.
use super::*;
use crate::mobs::{MobTypes, MobWorld};
use horizon_core::random::LegacyRandom;
use std::cell::RefCell;

const COW: &str = "minecraft:cow";
const MOD_COW: &str = "test:cow";

/// Grass at y 63 from x and z -64 to 63 under an open sky, a player at its middle: the level
/// natural spawning reads, its biome's creatures `entity` alone. `test:cow` is a mod's type whose
/// mod answers for it as vanilla's cow, unless `answer` says what its spawns come to.
struct Meadow<'a, 'w> {
    w: &'a MobWorld<'w>,
    types: &'a MobTypes,
    spawns: HashMap<String, Vec<SpawnEntry>>,
    players: Vec<[f64; 3]>,
    structures: StructureSpawns,
    /// The boxes of the mobs spawned so far (the level's entities).
    joined: RefCell<Vec<Aabb>>,
    /// Each mod spawn asked: whether its pack's first, and the pack's size with it.
    asked: RefCell<Vec<(bool, u32)>>,
    answer: Option<ModSpawned>,
}
impl<'a, 'w> Meadow<'a, 'w> {
    fn new(w: &'a MobWorld<'w>, types: &'a MobTypes, entity: &str) -> Self {
        let entry = SpawnEntry {
            entity: entity.to_owned(),
            weight: 8,
            min: 2,
            max: 4,
            constant: false,
        };
        Self {
            w,
            types,
            spawns: HashMap::from([("creature".to_owned(), vec![entry])]),
            players: vec![[0.5, 64.0, 0.5]],
            structures: StructureSpawns::default(),
            joined: RefCell::default(),
            asked: RefCell::default(),
            answer: None,
        }
    }
    fn cow(&self) -> MobType {
        self.mob_type(COW).expect("vanilla's cow")
    }
}
impl SpawnLevel for Meadow<'_, '_> {
    fn state(&self, x: i32, y: i32, z: i32) -> horizon_world::registry::BlockStateId {
        self.w.state(x, y, z)
    }
    fn facts(&self) -> &crate::blocks::BlockFacts {
        SpawnLevel::facts(self.w)
    }
    fn mob_data(&self) -> &super::super::data::MobData {
        SpawnLevel::mob_data(self.w)
    }
    fn block_tag(&self, tag: &str, block: &str) -> bool {
        self.w.block_tag(tag, block)
    }
    fn biome_tag(&self, _: &str, _: i32, _: i32, _: i32) -> bool {
        false
    }
    fn sky_light(&self, _: i32, y: i32, _: i32) -> i32 {
        if y > 63 { 15 } else { 0 }
    }
    fn block_light(&self, _: i32, _: i32, _: i32) -> i32 {
        0
    }
    fn sky_darken(&self) -> i32 {
        0
    }
    fn thundering(&self) -> bool {
        false
    }
    fn sea_level(&self) -> i32 {
        63
    }
    fn world_surface(&self, _: i32, _: i32) -> i32 {
        64
    }
    fn peaceful(&self) -> bool {
        false
    }
    fn dimension(&self) -> spawn_rules::Dimension {
        spawn_rules::Dimension::Overworld
    }
    fn surface_slime_chance(&self, _: i32, _: i32, _: i32) -> f32 {
        0.0
    }
    fn world_seed(&self) -> i64 {
        0
    }
    fn player_near(&self, _: f64, _: f64, _: f64, _: f64) -> bool {
        false
    }
}
impl NaturalLevel for Meadow<'_, '_> {
    fn min_y(&self) -> i32 {
        -64
    }
    fn max_y(&self) -> i32 {
        319
    }
    fn ambient_light(&self) -> f32 {
        0.0
    }
    fn has_ceiling(&self) -> bool {
        false
    }
    fn mob_type(&self, entity: &str) -> Option<MobType> {
        let t = self.types.types.get(entity)?;
        Some((
            t.species,
            t.width as f32,
            t.height as f32,
            Category::from_name(&t.category),
        ))
    }
    fn biome_spawns(&self, _: i32, _: i32, _: i32) -> Option<&HashMap<String, Vec<SpawnEntry>>> {
        Some(&self.spawns)
    }
    fn spawn_cost(&self, _: &str, _: i32, _: i32, _: i32) -> Option<SpawnCost> {
        None
    }
    fn creature_probability(&self, _: i32, _: i32, _: i32) -> f32 {
        0.75
    }
    fn structures_at(&self, _: i32, _: i32, _: i32, _: &dyn Fn(&str) -> bool) -> Vec<StructureAt> {
        Vec::new()
    }
    fn structure_spawns(&self) -> &StructureSpawns {
        &self.structures
    }
    fn nearest_player_positions(&self) -> &[[f64; 3]] {
        &self.players
    }
    fn respawn(&self) -> Option<[i32; 3]> {
        None
    }
    fn can_spawn_in_chunk(&self, _: i32, _: i32) -> bool {
        true
    }
    fn no_collision(&self, bounds: Aabb) -> bool {
        bounds.min_position().y >= 64.0
    }
    fn unobstructed(&self, bounds: Aabb) -> bool {
        !self.joined.borrow().iter().any(|b| b.intersects(bounds))
    }
    fn spawn_mobs(&self) -> bool {
        true
    }
    fn heightmap(&self, _: Heightmap, _: i32, _: i32) -> i32 {
        64
    }
    fn pathfindable_land(&self, x: i32, y: i32, z: i32) -> bool {
        self.mob_data().state(self.state(x, y, z)).flags & super::super::data::LAND != 0
    }
    fn mod_mob_type(&self, entity: &str) -> Option<ModMobType> {
        let (_, width, height, category) = self.cow();
        (entity == MOD_COW).then(|| ModMobType {
            category,
            width,
            height,
            spawn_scale: 1.0,
            fire_immune: false,
            far_from_player: true,
            summonable: true,
            placement: self.mob_data().placement(COW),
            heightmap: Heightmap::from_name(self.mob_data().heightmap(COW)),
        })
    }
    fn mod_spawn_rules(&self, entity: &str, reason: Reason, [x, y, z]: [i32; 3]) -> bool {
        // An animal's predicate draws nothing.
        let mut unused = LegacyRandom::new(0);
        entity == MOD_COW
            && spawn_rules::check_spawn_rules(Species::Cow, self, reason, &mut unused, x, y, z)
    }
    fn spawn_mod_mob(&self, spawn: &ModSpawn) -> ModSpawned {
        self.asked.borrow_mut().push((spawn.first, spawn.group_size));
        if let Some(answer) = self.answer {
            return answer;
        }
        let (_, width, height, category) = self.cow();
        let [x, y, z] = spawn.position;
        let [bx, by, bz] = [x, y, z].map(|c| c.floor() as i32);
        let bounds = spawn_box(x, y, z, width, height);
        let despawn = f64::from(category.despawn_distance().pow(2));
        let far = spawn
            .distance
            .is_some_and(|d| d > despawn && removed_when_far(Species::Cow));
        // A generation region holds no entities.
        let joined = self.joined.borrow().clone();
        let others = (spawn.reason == Reason::Natural).then_some(&joined[..]);
        if far
            || !mob_spawn_rules(Species::Cow, self, bx, by, bz)
            || !spawn_obstruction(Species::Cow, self, bounds, others, bx, by, bz)
        {
            return ModSpawned::Refused;
        }
        self.joined.borrow_mut().push(bounds);
        ModSpawned::Joined {
            bounds,
            max_cluster: max_cluster_size(Species::Cow),
            group_full: false,
        }
    }
}

/// The game's own spawn of `r`: its box joins the level.
fn add(l: &Meadow, r: &SpawnRequest) -> Option<Spawned> {
    let (_, width, height, _) = l.mob_type(r.entity)?;
    let [x, y, z] = r.position;
    let bounds = spawn_box(x, y, z, width, height);
    l.joined.borrow_mut().push(bounds);
    Some(Spawned {
        bounds,
        school: true,
    })
}

/// Natural spawning's packs from a few starts on the surface, 30 to 60 blocks from the player.
fn natural(l: &Meadow) -> Vec<Event> {
    let mut random = LegacyRandom::new(135);
    let player = SpawnPlayer {
        position: l.players[0],
        spectator: false,
    };
    let mut state = SpawnState::new(l, 289, vec![player], &[], |_, _| true);
    let mut events = Vec::new();
    for start in [[40, 64, 8], [-40, 64, 30], [10, 64, -45], [30, 64, 30], [-35, 64, -35]] {
        let chunk = (start[0] >> 4, start[2] >> 4);
        let mut trace = |e: Event| events.push(e);
        let mut spawn = add;
        spawn_category_for_position(
            l,
            &mut random,
            &mut state,
            Category::Creature,
            chunk,
            start,
            &mut spawn,
            &mut trace,
        );
    }
    events
}

/// Chunk generation's packs in a few chunks.
fn generated(l: &Meadow) -> Vec<GenEvent> {
    let mut random = LegacyRandom::new(135);
    let mut rules = LegacyRandom::new(136);
    let mut events = Vec::new();
    for (cx, cz) in [(0, 0), (1, -2), (-3, 2), (2, 1)] {
        let mut trace = |e: GenEvent| events.push(e);
        let mut spawn = |l: &Meadow, r: &SpawnRequest, _: &mut LegacyRandom| add(l, r).is_some();
        spawn_mobs_for_chunk_generation(
            l,
            [cx * 16, 319, cz * 16],
            (cx, cz),
            &mut random,
            &mut rules,
            &mut spawn,
            &mut trace,
        );
    }
    events
}

fn as_cow(e: Event) -> Event {
    match e {
        Event::Test(id, p, passed) if id == MOD_COW => Event::Test(COW.to_owned(), p, passed),
        Event::Spawn(id, p, yaw) if id == MOD_COW => Event::Spawn(COW.to_owned(), p, yaw),
        e => e,
    }
}

fn meadow(world: &mut crate::world::World, registry: &horizon_world::registry::BlockRegistry) {
    let grass = registry.default_state("minecraft:grass_block").unwrap();
    for cx in -4..4 {
        for cz in -4..4 {
            world
                .chunks
                .insert((cx, cz), std::sync::Arc::new(crate::chunk::Chunk::empty(cx, cz)));
        }
    }
    for x in -64..64 {
        for z in -64..64 {
            world.set(x, 63, z, grass).unwrap();
        }
    }
}

#[test]
fn a_mods_type_spawns_naturally_as_the_games_own_would() {
    crate::mobs::world::with_built_test_world(&[], meadow, |w, types| {
        let vanilla = Meadow::new(w, types, COW);
        let expected = natural(&vanilla);
        let modded = Meadow::new(w, types, MOD_COW);
        let got: Vec<Event> = natural(&modded).into_iter().map(as_cow).collect();
        assert_eq!(got, expected);
        let spawned = got.iter().filter(|e| matches!(e, Event::Spawn(..))).count();
        assert!(spawned >= 6, "{spawned} spawned: {got:?}");
        // A pack's first mob starts its group data; the pack grows by one with each that joins
        // (one refused, as vanilla's cow was, asks again at the same size).
        let asked = modded.asked.borrow();
        assert!(asked.len() > spawned, "{asked:?}");
        assert!(asked.iter().all(|&(first, size)| first == (size == 1)));
        assert!(asked.iter().any(|&(_, size)| size > 1), "{asked:?}");
    });
}

#[test]
fn a_mods_type_spawns_at_chunk_generation_as_the_games_own_would() {
    crate::mobs::world::with_built_test_world(&[], meadow, |w, types| {
        let expected = generated(&Meadow::new(w, types, COW));
        let modded = Meadow::new(w, types, MOD_COW);
        let got: Vec<GenEvent> = generated(&modded)
            .into_iter()
            .map(|e| match e {
                GenEvent::Spawn(id, p, yaw) if id == MOD_COW => GenEvent::Spawn(COW.into(), p, yaw),
                e => e,
            })
            .collect();
        assert_eq!(got, expected);
        let spawned = got.iter().filter(|e| matches!(e, GenEvent::Spawn(..))).count();
        assert!(spawned >= 4, "{spawned} spawned: {got:?}");
    });
}

/// A type whose mod makes no mob ends natural spawning's pass at once (`getMobForSpawn` gave
/// null: `spawnCategoryForPosition` returns), and has chunk generation try the same column again
/// without moving, four times for each mob of its pack.
#[test]
fn a_mob_its_mod_cannot_make_ends_the_pass() {
    crate::mobs::world::with_built_test_world(&[], meadow, |w, types| {
        let mut modded = Meadow::new(w, types, MOD_COW);
        modded.answer = Some(ModSpawned::Uncreated);
        let events = natural(&modded);
        // Each of the five starts ends at its first test that passes.
        let tests: Vec<usize> = events
            .iter()
            .enumerate()
            .filter(|(_, e)| matches!(e, Event::Test(_, _, true)))
            .map(|(i, _)| i)
            .collect();
        assert_eq!(tests.len(), 5, "{events:?}");
        assert_eq!(modded.asked.borrow().len(), 5);
        for (n, &i) in tests.iter().enumerate() {
            let next = events.get(i + 1);
            assert!(next.is_none() || n + 1 < tests.len(), "{events:?}");
            // What follows a start's end is the next start's first try.
            assert!(next.is_none_or(|e| matches!(e, Event::Try(..))), "{next:?}");
        }
        assert!(!events.iter().any(|e| matches!(e, Event::Spawn(..))));

        let mut modded = Meadow::new(w, types, MOD_COW);
        modded.answer = Some(ModSpawned::Uncreated);
        let mut random = LegacyRandom::new(7);
        let mut rules = LegacyRandom::new(8);
        let mut columns = Vec::new();
        let mut trace = |e: GenEvent| columns.push(e);
        let mut spawn = |_: &Meadow, _: &SpawnRequest, _: &mut LegacyRandom| true;
        // One chunk with packs certain to come (probability 0.75, seed 7 draws one first).
        spawn_mobs_for_chunk_generation(
            &modded,
            [0, 319, 0],
            (0, 0),
            &mut random,
            &mut rules,
            &mut spawn,
            &mut trace,
        );
        let asked = modded.asked.borrow().len();
        assert!(asked >= 8, "{asked} asked: {columns:?}");
        // Every try of a pack is at its first column: a pack is 2 to 4 mobs of 4 tries each.
        assert_eq!(columns.len(), asked);
        let first = columns.first().cloned();
        let same = columns.iter().take_while(|&c| Some(c) == first.as_ref()).count();
        assert!(same >= 8 && same % 4 == 0, "{columns:?}");
    });
}
