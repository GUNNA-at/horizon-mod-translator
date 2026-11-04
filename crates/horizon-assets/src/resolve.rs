//! Cuboid parent inheritance and texture-slot resolution.
use crate::{json::*, model::*};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

pub const MISSING_MODEL: &str = "minecraft:builtin/missing";
/// `ItemModelGenerator.GENERATED_ITEM_MODEL_ID`.
pub const GENERATED_MODEL: &str = "minecraft:builtin/generated";

/// Decoded models are immutable and their inherited geometry is shared.
pub struct ModelLibrary {
    models: BTreeMap<String, Arc<CuboidModel>>,
    missing: Arc<CuboidModel>,
}
#[derive(Clone, Debug)]
pub struct ResolvedCuboidModel {
    pub ambient_occlusion: bool,
    pub gui_light: GuiLight,
    pub display: ItemTransforms,
    pub textures: BTreeMap<String, Material>,
    /// Cyclic and dangling aliases are absent from `textures`.
    pub unresolved_textures: BTreeSet<String>,
    geometry: Option<Arc<CuboidModel>>,
    generated: bool,
    /// A composite geometry's children, each resolved through its own parents.
    children: Vec<ResolvedCuboidModel>,
}
impl ResolvedCuboidModel {
    /// Whether the top geometry is `builtin/generated`'s layer extrusion
    /// (no model closer to the leaf has elements of its own).
    pub fn uses_item_generator(&self) -> bool {
        self.generated
    }
    /// The OBJ geometry the model draws (NeoForge's OBJ loader), when that is its geometry.
    pub fn obj(&self) -> Option<&ObjGeometry> {
        self.geometry.as_ref().and_then(|m| m.obj.as_ref())
    }
    /// The child models a composite geometry draws together (NeoForge's composite loader).
    pub fn children(&self) -> &[ResolvedCuboidModel] {
        &self.children
    }
    pub fn elements(&self) -> &[Option<Element>] {
        self.geometry
            .as_ref()
            .and_then(|m| m.elements.as_deref())
            .unwrap_or(&[])
    }
    /// Native slot lookup strips a single leading '#'. Non-reference names
    /// are still slot names, not direct sprite identifiers.
    pub fn material(&self, slot: &str) -> Result<Option<&Material>> {
        if slot.is_empty() {
            return Err(error("empty texture lookup"));
        }
        Ok(self.textures.get(slot.strip_prefix('#').unwrap_or(slot)))
    }
}
impl ModelLibrary {
    /// A library that also knows `builtin/generated`, the parent of every
    /// flat item model; the native client registers it as a special model.
    pub fn with_item_generator(mut models: BTreeMap<String, CuboidModel>) -> Result<Self> {
        models.insert(GENERATED_MODEL.to_owned(), Self::item_generator_model());
        Self::new(models)
    }
    /// Register the item built-in while reusing already decoded resources.
    pub fn with_shared_item_generator(
        mut models: BTreeMap<String, Arc<CuboidModel>>,
    ) -> Result<Self> {
        models.insert(
            GENERATED_MODEL.to_owned(),
            Arc::new(Self::item_generator_model()),
        );
        Self::from_shared(models)
    }
    fn item_generator_model() -> CuboidModel {
        CuboidModel {
            parent: None,
            ambient_occlusion: None,
            gui_light: Some(GuiLight::Front),
            textures: BTreeMap::from([(
                "particle".to_owned(),
                TextureSlot::Reference("layer0".to_owned()),
            )]),
            elements: None,
            display: None,
            obj: None,
            children: None,
        }
    }
    pub fn new(models: BTreeMap<String, CuboidModel>) -> Result<Self> {
        Self::from_models(
            models
                .into_iter()
                .map(|(name, model)| (name, Arc::new(model))),
        )
    }
    /// Build an immutable resolution view without copying model geometry.
    pub fn from_shared(models: BTreeMap<String, Arc<CuboidModel>>) -> Result<Self> {
        Self::from_models(models)
    }
    fn from_models(models: impl IntoIterator<Item = (String, Arc<CuboidModel>)>) -> Result<Self> {
        let mut decoded = BTreeMap::new();
        for (name, model) in models {
            if decoded.insert(identifier(&name)?, model).is_some() {
                return Err(error("duplicate normalized model identifier"));
            }
        }
        let missing = Arc::new(missing_model());
        // The native discovery registers this built-in before consulting resources.
        decoded.insert(MISSING_MODEL.to_owned(), missing.clone());
        Ok(Self {
            models: decoded,
            missing,
        })
    }
    /// Follow the complete parent chain. A missing resource uses the built-in
    /// model; a cyclic chain is rejected, as native discovery excludes it.
    /// The returned model can outlive this library without copying its elements.
    pub fn resolve(&self, name: &str) -> Result<ResolvedCuboidModel> {
        let name = identifier(name)?;
        let model = self.models.get(&name).unwrap_or(&self.missing);
        self.resolve_from(model, name, 0)
    }
    /// [`Self::resolve`] from `first`, a model in the library or a composite's child (named after
    /// its model, `#` and its name); `depth` counts the composites around it.
    fn resolve_from(&self, first: &Arc<CuboidModel>, first_name: String, depth: usize) -> Result<ResolvedCuboidModel> {
        let mut visited = BTreeSet::new();
        let mut chain = vec![first];
        let mut names = vec![first_name.clone()];
        visited.insert(first_name);
        let mut parent = first.parent.clone();
        while let Some(name) = parent {
            if !visited.insert(name.clone()) {
                return Err(error("cyclic model parent"));
            }
            let model = self.models.get(&name).unwrap_or(&self.missing);
            chain.push(model);
            names.push(name);
            parent = model.parent.clone();
        }
        let ambient_occlusion = chain
            .iter()
            .find_map(|m| m.ambient_occlusion)
            .unwrap_or(true);
        let gui_light = chain
            .iter()
            .find_map(|m| m.gui_light)
            .unwrap_or(GuiLight::Side);
        let has_geometry = |m: &CuboidModel| m.elements.is_some() || m.obj.is_some() || m.children.is_some();
        let found = chain.iter().zip(&names).find(|(m, _)| has_geometry(m));
        let geometry = found.map(|(m, _)| Arc::clone(m));
        let mut children = Vec::new();
        if let Some((m, name)) = found
            && let Some(list) = &m.children
        {
            if depth > 16 {
                return Err(error("composite models nested too deep"));
            }
            for (child, model) in list {
                children.push(self.resolve_from(model, format!("{name}#{child}"), depth + 1)?);
            }
        }
        let generated = chain
            .iter()
            .zip(&names)
            .find(|(m, name)| has_geometry(m) || *name == GENERATED_MODEL)
            .is_some_and(|(_, name)| name == GENERATED_MODEL);
        let display = ItemTransforms {
            contexts: std::array::from_fn(|index| {
                chain
                    .iter()
                    .filter_map(|m| m.display.as_ref())
                    .map(|d| &d.contexts[index])
                    .find(|t| **t != DisplayTransform::Inherit)
                    .cloned()
                    .unwrap_or(DisplayTransform::Inherit)
            }),
        };
        // Resolve aliases only after applying every child override. A reference
        // inherited from a parent binds to the child's final slot definition.
        let mut slots = BTreeMap::new();
        for model in chain.into_iter().rev() {
            slots.extend(model.textures.iter().map(|(k, v)| (k.clone(), v)));
        }
        let mut textures = BTreeMap::<String, Material>::new();
        let mut unresolved_textures = BTreeSet::<String>::new();
        for start in slots.keys() {
            let mut current = start.as_str();
            let mut seen = BTreeSet::new();
            let mut path = Vec::new();
            let material = loop {
                if let Some(material) = textures.get(current) {
                    break Some(material.clone());
                }
                if unresolved_textures.contains(current) || !seen.insert(current) {
                    break None;
                }
                let Some(slot) = slots.get(current) else {
                    break None;
                };
                path.push(current);
                match slot {
                    TextureSlot::Reference(target) => current = target,
                    TextureSlot::Material(material) => break Some(material.clone()),
                }
            };
            // Cache the entire walk, including failures, so long alias chains
            // and cycles are traversed once rather than once per referencing slot.
            match material {
                Some(material) => {
                    for name in path {
                        textures.insert(name.to_owned(), material.clone());
                    }
                }
                None => {
                    unresolved_textures.extend(path.into_iter().map(str::to_owned));
                }
            }
        }
        Ok(ResolvedCuboidModel {
            ambient_occlusion,
            gui_light,
            display,
            textures,
            unresolved_textures,
            geometry,
            generated,
            children,
        })
    }
}

/// The native missing-model cube uses slot names without a leading '#'.
pub fn missing_model() -> CuboidModel {
    let faces = Direction::ALL
        .into_iter()
        .map(|direction| {
            (
                direction,
                Some(Face {
                    cull: Some(direction),
                    tint_index: -1,
                    texture: "missingno".to_owned(),
                    uv: Some([0.0, 0.0, 16.0, 16.0]),
                    rotation: 0,
                }),
            )
        })
        .collect();
    CuboidModel {
        parent: None,
        ambient_occlusion: None,
        gui_light: None,
        elements: Some(vec![Some(Element {
            from: [0.0; 3],
            to: [16.0; 3],
            faces,
            rotation: None,
            shade_direction_override: None,
            light_emission: 0,
        })]),
        display: Some(ItemTransforms {
            contexts: std::array::from_fn(|_| DisplayTransform::Inherit),
        }),
        textures: BTreeMap::from([
            (
                "particle".to_owned(),
                TextureSlot::Reference("missingno".to_owned()),
            ),
            (
                "missingno".to_owned(),
                TextureSlot::Material(Material {
                    sprite: "minecraft:missingno".to_owned(),
                    force_translucent: false,
                }),
            ),
        ]),
        obj: None,
        children: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// NeoForge's composite loader, as Create's display link uses it: each child takes its own
    /// textures and parents, the block state's turn applies to all, a hidden child draws nothing.
    #[test]
    fn a_composites_children_resolve_on_their_own_and_bake_together() {
        use crate::bake::{BlockModelTransform, bake_block_model};
        let link = CuboidModel::from_json(
            r##"{"loader":"neoforge:composite","parent":"block/block","textures":{"particle":"block/stone"},
                "visibility":{"hidden":false},
                "children":{
                    "base":{"textures":{"0":"block/stone"},"elements":[{"from":[0,0,0],"to":[16,2,16],"faces":{"north":{"texture":"#0"}}}]},
                    "lamp":{"parent":"mod:block/lamp"},
                    "hidden":{"textures":{"0":"block/dirt"},"elements":[{"from":[0,0,0],"to":[16,16,16],"faces":{"up":{"texture":"#0"}}}]}}}"##,
        )
        .unwrap();
        let lamp = CuboidModel::from_json(r##"{"textures":{"glow":"block/glowstone"},"elements":[{"from":[4,2,4],"to":[12,6,12],"faces":{"up":{"texture":"#glow"}}}]}"##).unwrap();
        let block = CuboidModel::from_json(r#"{"ambientocclusion":true}"#).unwrap();
        assert_eq!(link.parents(), ["minecraft:block/block", "mod:block/lamp"]);
        let library = ModelLibrary::new(BTreeMap::from([
            ("mod:block/link".into(), link),
            ("mod:block/lamp".into(), lamp),
            ("minecraft:block/block".into(), block),
        ]))
        .unwrap();
        let model = library.resolve("mod:block/link").unwrap();
        assert_eq!(model.children().len(), 2);
        assert_eq!(model.material("particle").unwrap().unwrap().sprite, "minecraft:block/stone");
        // A quarter turn about y: the base's north face turns east.
        let quads = bake_block_model(&model, BlockModelTransform::new([0, 1, 0], false)).unwrap();
        let faces: Vec<_> = quads.iter().map(|q| (q.material.sprite.as_str(), q.facing)).collect();
        assert_eq!(faces, [("minecraft:block/stone", Direction::East), ("minecraft:block/glowstone", Direction::Up)]);
    }

    #[test]
    fn inherited_alias_uses_child_material_and_geometry_outlives_library() {
        let parent = CuboidModel::from_json(r##"{"textures":{"side":"#all","all":"block/stone"},"elements":[{"from":[0,0,0],"to":[16,8,16],"faces":{"up":{"texture":"#side"}}}]}"##).unwrap();
        let child = CuboidModel::from_json(r#"{"parent":"horizon:parent","textures":{"all":{"sprite":"block/glass","force_translucent":true}}}"#).unwrap();
        let library = ModelLibrary::new(BTreeMap::from([
            ("horizon:parent".into(), parent),
            ("horizon:child".into(), child),
        ]))
        .unwrap();
        let model = library.resolve("horizon:child").unwrap();
        drop(library);
        assert_eq!(model.elements()[0].as_ref().unwrap().to, [16.0, 8.0, 16.0]);
        assert_eq!(
            model.material("#side").unwrap().unwrap(),
            &Material {
                sprite: "minecraft:block/glass".into(),
                force_translucent: true
            }
        );
    }
    #[test]
    fn absent_parents_fall_back_but_cycles_invalidate_descendants() {
        let a = CuboidModel::from_json(r#"{"parent":"horizon:b"}"#).unwrap();
        let b = CuboidModel::from_json(r#"{"parent":"horizon:a"}"#).unwrap();
        let child = CuboidModel::from_json(r#"{"parent":"horizon:a","elements":[]}"#).unwrap();
        let library = ModelLibrary::new(BTreeMap::from([
            ("horizon:a".into(), a),
            ("horizon:b".into(), b),
            ("horizon:child".into(), child),
        ]))
        .unwrap();
        assert!(library.resolve("horizon:child").is_err());
        let absent = library.resolve("horizon:absent").unwrap();
        assert_eq!(absent.elements().len(), 1);
        assert_eq!(
            absent.material("particle").unwrap().unwrap().sprite,
            "minecraft:missingno"
        );
    }
}
