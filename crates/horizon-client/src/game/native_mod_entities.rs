//! The entities translated mods keep in their own runtime (Waddles' penguins, Create's contraptions
//! and seats), as the game's commands see them (docs/native-mods.md, "Mod entities in commands";
//! hook points of API @API:mod_entity_commands@). Their mods tell what they are each time a
//! selector that can find entities runs (`native_mod_entities`): it finds them by type, place,
//! rotation, name and tag as it finds the game's own, and `/kill`, `/tp` and `/data get entity`
//! act on them through their mod (`native_mod_entity_act`, `native_mod_entity_nbt`).
use super::{Game, commands::Target};
use horizon_world::nbt::Tag;
use std::sync::atomic::{AtomicI32, Ordering};

/// One of a translated mod's entities, as its mod tells it.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct ModEntity {
    /// Its network id: the mods' runtime numbers its entities from 2^30 up, past the game's own.
    pub id: i32,
    /// Its type (`waddles:adelie_penguin`).
    pub kind: String,
    /// The server level it is in (`minecraft:overworld`).
    pub dimension: String,
    /// Its feet.
    pub position: [f64; 3],
    /// Its yaw and pitch (`getYRot`, `getXRot`).
    pub rotation: [f32; 2],
    pub uuid: u128,
    /// Its box's width and height, and its eyes' height above its feet.
    pub size: [f32; 3],
    /// `Entity.isAlive`: not removed, and a living one's health above 0.
    pub alive: bool,
    /// Its custom name (a name tag's, `setCustomName`), when it has one.
    pub custom_name: Option<String>,
    /// `Entity.getTags`.
    pub tags: Vec<String>,
}

/// What a command does to a translated mod's entity.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ModEntityAct {
    /// `/kill`: `Entity.kill()`; a living one dies of `generic_kill`, with its death and drops.
    Kill,
    /// `/tp`: `Entity.teleportTo` the position, then the rotation (yaw, pitch) when one is given.
    Teleport {
        position: [f64; 3],
        rotation: Option<[f32; 2]>,
    },
}

/// A UUID's four ints, most significant first, as the game keeps them.
pub(super) fn uuid_ints(uuid: u128) -> [i32; 4] {
    [96, 64, 32, 0].map(|shift| (uuid >> shift) as u32 as i32)
}

/// The network id of an entity a native mod makes: one counter for every translated mod (their
/// hosts' `mod_entity_id`), from 2^30 up past the game's own ids, so two mods' entities never
/// share one, as vanilla's `Entity.ENTITY_COUNTER` is one for every entity. Only the mods' code
/// calls it.
#[allow(dead_code)]
pub(crate) fn next_mod_entity_id() -> i32 {
    static NEXT: AtomicI32 = AtomicI32::new(1 << 30);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

impl Game<'_> {
    /// Asks the mods for their entities in every loaded level (`native_mod_entities`), as a
    /// selector is about to run. None on a client of a server, whose mods' entities are copies.
    pub(super) fn refresh_mod_entities(&mut self) {
        self.mod_entities.clear();
        if self.remote.is_some() {
            return;
        }
        let mut found = Vec::new();
        for dim in self.loaded_dims() {
            let mut listed = Vec::new();
            self.native_mod_entities(dim.id(), &mut listed);
            for mut e in listed {
                e.dimension = dim.id().to_owned();
                if !found.iter().any(|f: &ModEntity| f.id == e.id) {
                    found.push(e);
                }
            }
        }
        self.mod_entities = found;
    }
    /// Mod entity `id` of the level in place, as its mod last told it.
    pub(super) fn mod_entity(&self, id: u32) -> Option<&ModEntity> {
        let here = self.dim.id();
        self.mod_entities
            .iter()
            .find(|e| u32::try_from(e.id).ok() == Some(id) && e.dimension == here)
    }
    /// The living mod entities of the level in place, as a selector's candidates.
    pub(super) fn mod_entity_candidates(&self) -> Vec<Target> {
        let here = self.dim.id();
        self.mod_entities
            .iter()
            .filter(|e| e.alive && e.dimension == here)
            .filter_map(|e| u32::try_from(e.id).ok().map(Target::Mod))
            .collect()
    }
    /// The mod entity of the level in place with `uuid`.
    pub(super) fn mod_entity_with_uuid(&self, uuid: [i32; 4]) -> Option<Target> {
        let here = self.dim.id();
        self.mod_entities
            .iter()
            .find(|e| e.dimension == here && uuid_ints(e.uuid) == uuid)
            .and_then(|e| u32::try_from(e.id).ok().map(Target::Mod))
    }
    /// Has the mods do `act` to mod entity `id` of the level in place (`native_mod_entity_act`):
    /// whether one did. A teleport moves the entity as the commands see it at once, so an
    /// `execute at` after it starts from where it went.
    pub(super) fn act_on_mod_entity(&mut self, id: u32, act: &ModEntityAct) -> bool {
        let Ok(network) = i32::try_from(id) else {
            return false;
        };
        let mut done = false;
        let dimension = self.dim.id();
        self.native_mod_entity_act(dimension, network, act, &mut done);
        if done
            && let ModEntityAct::Teleport { position, rotation } = act
            && let Some(e) = self
                .mod_entities
                .iter_mut()
                .find(|e| e.id == network && e.dimension == dimension)
        {
            e.position = *position;
            if let Some(r) = rotation {
                e.rotation = *r;
            }
        }
        done
    }
    /// Mod entity `id`'s compound as `/data get entity` reads it (`Entity.saveWithoutId`), from
    /// its mod (`native_mod_entity_nbt`).
    pub(super) fn mod_entity_nbt(&mut self, id: u32) -> Option<Tag> {
        let network = i32::try_from(id).ok()?;
        let mut nbt = None;
        self.native_mod_entity_nbt(self.dim.id(), network, &mut nbt);
        nbt
    }
}
