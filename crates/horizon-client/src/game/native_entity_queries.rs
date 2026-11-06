//! Entity queries for native mods (docs/native-mods.md, "Entities in a box"): what a mod's
//! `Level.getEntitiesOfClass` and `getEntities` find of the game's own entities.
use super::Game;
use std::collections::HashMap;
use std::sync::OnceLock;

impl Game<'_> {
    /// The network ids of the server level `dimension`'s entities whose box meets the box from
    /// `min` to `max` (`AABB.intersects`: touching faces aside), as `Level.getEntities` finds
    /// them: mobs, item entities, orbs, projectiles, armor stands, frames, paintings, boats,
    /// minecarts and the rest. Players aside (`player_entities` tells them). An entity's box is
    /// its type's size about its feet (`EntityType.getDimensions().makeBoundingBox`), a baby's
    /// as an adult's. Empty on a client or for another dimension.
    /// API @API:native_entities_in@.
    #[allow(dead_code)]
    pub(crate) fn native_entities_in(&self, dimension: &str, min: [f64; 3], max: [f64; 3]) -> Vec<i32> {
        if (self.serving.is_none() && self.options.dedicated.is_none()) || dimension != self.dim.id() {
            return Vec::new();
        }
        self.level_candidates()
            .into_iter()
            .filter_map(|t| {
                let p = self.target_position(t)?;
                let (width, height) = self.type_size(&self.target_type(t));
                // `EntityDimensions.makeBoundingBox`: half the width in floats.
                let half = f64::from(width / 2.0);
                let (lo, hi) = ([p[0] - half, p[1], p[2] - half], [p[0] + half, p[1] + f64::from(height), p[2] + half]);
                let meets = (0..3).all(|i| lo[i] < max[i] && hi[i] > min[i]);
                if meets { self.network_entity_id(t) } else { None }
            })
            .collect()
    }

    /// An entity type's width and height (`EntityType.getDimensions()`) from `entity-types.json`,
    /// read once; none for a type it lacks.
    fn type_size(&self, ty: &str) -> (f32, f32) {
        static SIZES: OnceLock<HashMap<String, (f32, f32)>> = OnceLock::new();
        SIZES
            .get_or_init(|| {
                std::fs::read(self.options.resources.join("entity-types.json"))
                    .ok()
                    .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
                    .and_then(|v| {
                        v.as_object().map(|o| {
                            o.iter()
                                .filter_map(|(k, t)| Some((k.clone(), (t["width"].as_f64()? as f32, t["height"].as_f64()? as f32))))
                                .collect()
                        })
                    })
                    .unwrap_or_default()
            })
            .get(ty)
            .copied()
            .unwrap_or((0.0, 0.0))
    }
}
