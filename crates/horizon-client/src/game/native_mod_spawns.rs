//! Natural spawning of the translated mods' mobs (docs/native-mods.md, "Mod mobs in natural
//! spawning"; hook points of API @API:mod_spawning@). A biome's spawns name a mod's entity type
//! once its pack's biome modifiers added it (`neoforge:add_spawns`); the game's spawner
//! (`mobs::spawn_selection`) then picks it as it picks vanilla's, and asks the mod whose type it is
//! for what only its code knows: the type's facts (`native_mod_spawn_types`), its spawn rules
//! (`native_mod_spawn_rules`), and the mob itself (`native_mod_spawn_mob`). The mods' own entities
//! count towards the spawn caps and stand in the way of new mobs (`native_mod_spawn_census`).
use super::Game;
use horizon_client::mobs::{
    data::Placement,
    spawn_rules::Reason,
    spawn_selection::{Category, Heightmap, ModMobType},
};
use std::collections::HashMap;

/// One of a translated mod's entity types as natural spawning reads it, as its mod tells it.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct ModSpawnType {
    /// `waddles:adelie_penguin`.
    pub id: String,
    /// Its category's serialized name (`creature`).
    pub category: String,
    pub width: f32,
    pub height: f32,
    /// `EntityType.spawnDimensionsScale`: how much bigger its spawn's box is than its own.
    pub spawn_scale: f32,
    pub fire_immune: bool,
    /// `EntityType.canSpawnFarFromPlayer`.
    pub far_from_player: bool,
    /// `EntityType.canSummon`.
    pub summonable: bool,
    /// Its placement type (`on_ground`, `in_water`, `in_lava`, `no_restrictions`), `custom` for a
    /// mod's own (whose test its spawn rules make), None where no mod registered one.
    pub placement: Option<String>,
    /// `SpawnPlacements.getHeightmapType` (`MOTION_BLOCKING_NO_LEAVES`).
    pub heightmap: String,
}

/// A mob of a translated mod's type that the spawner placed, for its mod to make.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ModSpawnRequest {
    pub entity: String,
    /// Its feet.
    pub position: [f64; 3],
    pub yaw: f32,
    /// The `MobSpawnType` constant (`NATURAL`, `CHUNK_GENERATION`).
    pub reason: &'static str,
    /// The first of its pack to join: the pack's group data starts empty.
    pub first: bool,
    /// The pack's size with it (`isMaxGroupSizeReached`).
    pub group_size: u32,
    /// The nearest player's squared distance (natural spawning's `isValidPositionForMob`); None at
    /// chunk generation.
    pub distance: Option<f64>,
    /// The boxes of the mobs this pass made that the game doesn't hold yet: they stand in the new
    /// mob's way (`checkSpawnObstruction`) as the game's own do.
    pub pending: Vec<[f64; 6]>,
}

/// What came of a [`ModSpawnRequest`]. Only the mods' code makes one.
#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ModSpawnOutcome {
    /// The type made no mob (`getMobForSpawn` gave null).
    Uncreated,
    /// The mob refused the place (too far, its rules, an obstruction): none joined.
    Refused,
    /// It was finalized and joined its mod's level: its box (min x, y, z, max x, y, z),
    /// `getMaxSpawnClusterSize`, and whether its pack is full (`isMaxGroupSizeReached`).
    Joined {
        bounds: [f64; 6],
        max_cluster: u32,
        group_full: bool,
    },
}

/// One of a translated mod's own entities as the spawn caps count it (`NaturalSpawner.createState`).
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct ModCensus {
    pub kind: String,
    /// Its category's serialized name; `misc` ones are not counted.
    pub category: String,
    /// `blockPosition()`.
    pub block: [i32; 3],
    pub mob: bool,
    /// `isPersistenceRequired() || requiresCustomPersistence()`.
    pub persistent: bool,
    /// Its box where it blocks building (`Entity.blocksBuilding`): it stands in a spawn's way.
    pub blocks: Option<[f64; 6]>,
}

/// The `MobSpawnType` constant a mod knows a spawn reason by.
pub(super) fn reason_name(reason: Reason) -> &'static str {
    match reason {
        Reason::Natural => "NATURAL",
        Reason::ChunkGeneration => "CHUNK_GENERATION",
        Reason::Spawner => "SPAWNER",
        Reason::TrialSpawner => "TRIAL_SPAWNER",
        Reason::Reinforcement => "REINFORCEMENT",
        Reason::Patrol => "PATROL",
        Reason::Structure => "STRUCTURE",
        Reason::Event => "EVENT",
        Reason::Command => "COMMAND",
        Reason::SpawnItemUse => "SPAWN_EGG",
        Reason::MobSummoned => "MOB_SUMMONED",
        Reason::Conversion => "CONVERSION",
        Reason::Breeding => "BREEDING",
        Reason::Triggered => "TRIGGERED",
        Reason::Jockey => "JOCKEY",
        Reason::Dispenser => "DISPENSER",
        Reason::Bucket => "BUCKET",
        Reason::Load => "LOAD",
        Reason::DimensionTravel => "DIMENSION_TRAVEL",
    }
}

/// A mod's type as the spawner reads it. A placement type of the mod's own spawns anywhere here:
/// its test is part of the type's spawn rules, which its mod answers.
pub(super) fn mob_type(t: &ModSpawnType) -> ModMobType {
    ModMobType {
        category: Category::from_name(&t.category),
        width: t.width,
        height: t.height,
        spawn_scale: t.spawn_scale,
        fire_immune: t.fire_immune,
        far_from_player: t.far_from_player,
        summonable: t.summonable,
        placement: match t.placement.as_deref() {
            Some("on_ground") => Placement::OnGround,
            Some("in_water") => Placement::InWater,
            Some("in_lava") => Placement::InLava,
            _ => Placement::NoRestrictions,
        },
        heightmap: Heightmap::from_name(&t.heightmap),
    }
}

impl Game<'_> {
    /// The translated mods' entity types natural spawning may make, by id: asked of the mods once
    /// (`native_mod_spawn_types`), the first time a spawn meets a type the game doesn't know.
    pub(super) fn mod_spawn_types(&self) -> &HashMap<String, ModMobType> {
        self.mod_spawn_types.get_or_init(|| {
            let mut types = Vec::new();
            self.native_mod_spawn_types(&mut types);
            types.iter().map(|t| (t.id.clone(), mob_type(t))).collect()
        })
    }
    /// The translated mods' own entities in the level in place, as a spawn pass counts them
    /// (`native_mod_spawn_census`). None on a client of a server.
    pub(super) fn mod_spawn_census(&self) -> Vec<ModCensus> {
        let mut census = Vec::new();
        if self.remote.is_none() {
            self.native_mod_spawn_census(self.dim.id(), &mut census);
        }
        census
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mods_type_reads_as_the_spawner_needs() {
        let penguin = ModSpawnType {
            id: "waddles:adelie_penguin".into(),
            category: "creature".into(),
            width: 0.4,
            height: 0.95,
            spawn_scale: 1.0,
            far_from_player: true,
            summonable: true,
            placement: Some("on_ground".into()),
            heightmap: "MOTION_BLOCKING_NO_LEAVES".into(),
            ..Default::default()
        };
        let t = mob_type(&penguin);
        assert_eq!(t.category, Category::Creature);
        assert_eq!(t.placement, Placement::OnGround);
        assert_eq!(t.heightmap, Heightmap::MotionBlockingNoLeaves);
        // A placement type of the mod's own, and none at all, put no limit on the block.
        for placement in [Some("custom".to_owned()), None] {
            let t = mob_type(&ModSpawnType {
                placement,
                heightmap: "OCEAN_FLOOR".into(),
                category: "water_creature".into(),
                ..penguin.clone()
            });
            assert_eq!(t.placement, Placement::NoRestrictions);
            assert_eq!(t.heightmap, Heightmap::OceanFloor);
            assert_eq!(t.category, Category::WaterCreature);
        }
    }
}
