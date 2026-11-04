//! The 26.3 cuboid model format, before parent resolution and quad baking.
//!
//! Absent geometry differs from an explicit empty element list. Explicit null
//! elements, faces and display transforms are retained as the native decoder
//! does; render preparation must diagnose these before baking.
use crate::json::*;
use serde_json::value::RawValue;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Direction {
    Down,
    Up,
    North,
    South,
    West,
    East,
}
impl Direction {
    pub const ALL: [Self; 6] = [
        Self::Down,
        Self::Up,
        Self::North,
        Self::South,
        Self::West,
        Self::East,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Self::Down => "down",
            Self::Up => "up",
            Self::North => "north",
            Self::South => "south",
            Self::West => "west",
            Self::East => "east",
        }
    }
    pub fn by_name(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|direction| direction.name() == name)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuiLight {
    Front,
    Side,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Material {
    pub sprite: String,
    pub force_translucent: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextureSlot {
    Reference(String),
    Material(Material),
}
#[derive(Clone, Debug, PartialEq)]
pub struct CuboidModel {
    pub parent: Option<String>,
    pub ambient_occlusion: Option<bool>,
    pub gui_light: Option<GuiLight>,
    pub textures: BTreeMap<String, TextureSlot>,
    pub elements: Option<Vec<Option<Element>>>,
    pub display: Option<ItemTransforms>,
    /// The geometry NeoForge's OBJ loader reads instead of elements.
    pub obj: Option<ObjGeometry>,
    /// NeoForge's composite loader (`"loader": "neoforge:composite"`): named child models, each with
    /// its own textures and parents, drawn together; a child its `visibility` hides is left out.
    pub children: Option<Vec<(String, std::sync::Arc<CuboidModel>)>>,
}
/// A model NeoForge's OBJ loader reads (`"loader": "neoforge:obj"`, `forge:obj` before NeoForge):
/// the mesh's location and the loader's options. The mesh is read beside the model; whoever loads
/// models attaches it ([`crate::obj`]), and a model without one draws nothing.
#[derive(Clone, Debug, PartialEq)]
pub struct ObjGeometry {
    /// `create:models/block/water_wheel/water_wheel.obj`.
    pub model: String,
    /// `flip_v`: texture v counts from the top, as Blender writes it.
    pub flip_v: bool,
    /// `automatic_culling` (on unless off): a face flat on a side of the block is culled there.
    pub automatic_culling: bool,
    /// `emissive_ambient` (on unless off): a material's `Ka` lights its faces.
    pub emissive_ambient: bool,
    /// `mtl_override`: the material library to use instead of the file's `mtllib`.
    pub mtl_override: Option<String>,
    pub mesh: Option<std::sync::Arc<crate::obj::ObjMesh>>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Element {
    pub from: [f32; 3],
    pub to: [f32; 3],
    pub faces: BTreeMap<Direction, Option<Face>>,
    pub rotation: Option<ElementRotation>,
    pub shade_direction_override: Option<Direction>,
    pub light_emission: u8,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Face {
    pub cull: Option<Direction>,
    pub tint_index: i32,
    pub texture: String,
    pub uv: Option<[f32; 4]>,
    /// Normalized counterclockwise quadrants, in the range 0..4.
    pub rotation: u8,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    X,
    Y,
    Z,
}
#[derive(Clone, Debug, PartialEq)]
pub enum RotationValue {
    SingleAxis { axis: Axis, angle: f32 },
    EulerXyz([f32; 3]),
}
#[derive(Clone, Debug, PartialEq)]
pub struct ElementRotation {
    /// Block units, converted from model coordinates during decoding.
    pub origin: [f32; 3],
    pub value: RotationValue,
    pub rescale: bool,
}
#[derive(Clone, Debug, PartialEq)]
pub struct ItemTransform {
    pub rotation: [f32; 3],
    pub translation: [f32; 3],
    pub scale: [f32; 3],
}
/// Missing transforms inherit later. Explicit identity and null values do not.
#[derive(Clone, Debug, PartialEq)]
pub enum DisplayTransform {
    Inherit,
    Explicit(Option<ItemTransform>),
}
pub const DISPLAY_CONTEXTS: [&str; 9] = [
    "thirdperson_lefthand",
    "thirdperson_righthand",
    "firstperson_lefthand",
    "firstperson_righthand",
    "head",
    "gui",
    "ground",
    "fixed",
    "on_shelf",
];
#[derive(Clone, Debug, PartialEq)]
pub struct ItemTransforms {
    pub contexts: [DisplayTransform; 9],
}

impl CuboidModel {
    /// The models this one needs besides itself: its parent, and its composite children's parents.
    pub fn parents(&self) -> Vec<&str> {
        let mut out: Vec<&str> = self.parent.as_deref().into_iter().collect();
        for (_, child) in self.children.iter().flatten() {
            out.extend(child.parents());
        }
        out
    }
    /// Decode strict JSON, preserving the native parser's primitive coercions,
    /// optional inheritance fields, nullable children and f32 conversion.
    /// Like the native stream entry point, this reads one value and leaves
    /// trailing input unread. The pinned native entry point uses strict JSON.
    pub fn from_json(json: &str) -> Result<Self> {
        let raw = serde_json::Deserializer::from_str(json.strip_prefix('\u{feff}').unwrap_or(json))
            .into_iter::<&RawValue>()
            .next()
            .ok_or_else(|| error("empty model input"))??;
        validate_number_tokens(raw)?;
        Self::decode(raw)
    }
    fn decode(raw: &RawValue) -> Result<Self> {
        let obj = object(raw)?;
        let parent = obj
            .get("parent")
            .map(|r| primitive_string(r))
            .transpose()?
            .unwrap_or_default();
        let parent = if parent.is_empty() {
            None
        } else {
            Some(identifier(&parent)?)
        };
        let ambient_occlusion = obj
            .get("ambientocclusion")
            .map(|r| boolean(r))
            .transpose()?;
        let gui_light = obj
            .get("gui_light")
            .map(|raw| match primitive_string(raw)?.as_str() {
                "front" => Ok(GuiLight::Front),
                "side" => Ok(GuiLight::Side),
                _ => Err(error("invalid gui light")),
            })
            .transpose()?;
        let mut textures = BTreeMap::new();
        if let Some(raw) = obj.get("textures") {
            for (key, raw) in object(raw)? {
                textures.insert(key, texture(raw)?);
            }
        }
        let elements = obj
            .get("elements")
            .map(|raw| {
                array(raw)?
                    .into_iter()
                    .map(|raw| nullable(raw, Element::decode))
                    .collect::<Result<Vec<_>>>()
            })
            .transpose()?;
        let display = obj
            .get("display")
            .map(|r| ItemTransforms::decode(r))
            .transpose()?;
        let loader = obj.get("loader").map(|r| primitive_string(r)).transpose()?;
        let option = |key: &str, default: bool| obj.get(key).map(|r| boolean(r)).transpose().map(|v| v.unwrap_or(default));
        let obj_geometry = match loader.as_deref() {
            Some("neoforge:obj" | "forge:obj") => Some(ObjGeometry {
                model: identifier(&string(required(&obj, "model")?)?)?,
                flip_v: option("flip_v", false)?,
                automatic_culling: option("automatic_culling", true)?,
                emissive_ambient: option("emissive_ambient", true)?,
                mtl_override: obj.get("mtl_override").map(|r| string(r)).transpose()?,
                mesh: None,
            }),
            _ => None,
        };
        let children = match loader.as_deref() {
            Some("neoforge:composite" | "forge:composite") => {
                let mut hidden = BTreeMap::new();
                if let Some(raw) = obj.get("visibility") {
                    for (name, raw) in object(raw)? {
                        hidden.insert(name, !boolean(raw)?);
                    }
                }
                let mut children = Vec::new();
                for (name, raw) in object(required(&obj, "children")?)? {
                    if hidden.get(&name) != Some(&true) {
                        children.push((name, std::sync::Arc::new(Self::decode(raw)?)));
                    }
                }
                Some(children)
            }
            _ => None,
        };
        Ok(Self {
            parent,
            ambient_occlusion,
            gui_light,
            textures,
            elements,
            display,
            obj: obj_geometry,
            children,
        })
    }
}
fn nullable<T>(raw: &RawValue, decode: impl FnOnce(&RawValue) -> Result<T>) -> Result<Option<T>> {
    if raw.get() == "null" {
        Ok(None)
    } else {
        decode(raw).map(Some)
    }
}
fn texture(raw: &RawValue) -> Result<TextureSlot> {
    if raw.get().starts_with('"') {
        let name = string(raw)?;
        if name.is_empty() {
            return Err(error("empty texture slot value"));
        }
        if let Some(target) = name.strip_prefix('#') {
            return Ok(TextureSlot::Reference(target.to_owned()));
        }
        return Ok(TextureSlot::Material(Material {
            sprite: identifier(&name)?,
            force_translucent: false,
        }));
    }
    let obj = object(raw)?;
    let sprite = identifier(&string(required(&obj, "sprite")?)?)?;
    let force_translucent = match obj.get("force_translucent") {
        None => false,
        Some(raw) if raw.get() == "null" => false,
        Some(raw) if raw.get() == "true" => true,
        Some(raw) if raw.get() == "false" => false,
        Some(_) => return Err(error("invalid material boolean")),
    };
    Ok(TextureSlot::Material(Material {
        sprite,
        force_translucent,
    }))
}
impl Element {
    fn decode(raw: &RawValue) -> Result<Self> {
        let obj = object(raw)?;
        let from = vector(required(&obj, "from")?)?;
        let to = vector(required(&obj, "to")?)?;
        if from.iter().chain(&to).any(|v| !(-16.0..=32.0).contains(v)) {
            return Err(error("element outside model bounds"));
        }
        let rotation = obj
            .get("rotation")
            .map(|r| ElementRotation::decode(r))
            .transpose()?;
        let mut faces = BTreeMap::new();
        for (name, raw) in object(required(&obj, "faces")?)? {
            let direction =
                Direction::by_name(&name).ok_or_else(|| error("invalid face direction"))?;
            faces.insert(direction, nullable(raw, Face::decode)?);
        }
        if faces.is_empty() {
            return Err(error("element requires a face"));
        }
        let shade_direction_override = obj
            .get("shade_direction_override")
            .map(|raw| {
                Direction::by_name(&primitive_string(raw)?)
                    .ok_or_else(|| error("invalid shade direction"))
            })
            .transpose()?;
        let light_emission = obj
            .get("light_emission")
            .map(|r| integer(r))
            .transpose()?
            .unwrap_or(0);
        if !(0..=15).contains(&light_emission) {
            return Err(error("invalid light emission"));
        }
        Ok(Self {
            from,
            to,
            faces,
            rotation,
            shade_direction_override,
            light_emission: light_emission as u8,
        })
    }
}
impl Face {
    fn decode(raw: &RawValue) -> Result<Self> {
        let obj = object(raw)?;
        let cull = obj
            .get("cullface")
            .map(|r| primitive_string(r))
            .transpose()?
            .and_then(|s| Direction::by_name(&s));
        let texture = primitive_string(required(&obj, "texture")?)?;
        let tint_index = obj
            .get("tintindex")
            .map(|r| integer(r))
            .transpose()?
            .unwrap_or(-1);
        let rotation = obj
            .get("rotation")
            .map(|r| integer(r))
            .transpose()?
            .unwrap_or(0)
            .rem_euclid(360);
        if rotation % 90 != 0 {
            return Err(error("face rotation must be a multiple of 90"));
        }
        let uv = obj.get("uv").map(|r| vector(r)).transpose()?;
        Ok(Self {
            cull,
            tint_index,
            texture,
            uv,
            rotation: (rotation / 90) as u8,
        })
    }
}
impl ElementRotation {
    fn decode(raw: &RawValue) -> Result<Self> {
        let obj = object(raw)?;
        let origin = vector::<3>(required(&obj, "origin")?)?.map(|v| v * 0.0625);
        let value = if obj.contains_key("axis") || obj.contains_key("angle") {
            let axis = match primitive_string(required(&obj, "axis")?)?
                .to_lowercase()
                .as_str()
            {
                "x" => Axis::X,
                "y" => Axis::Y,
                "z" => Axis::Z,
                _ => return Err(error("invalid rotation axis")),
            };
            RotationValue::SingleAxis {
                axis,
                angle: float(required(&obj, "angle")?)?,
            }
        } else if ["x", "y", "z"].iter().any(|key| obj.contains_key(*key)) {
            let mut angles = [0.0; 3];
            for (angle, key) in angles.iter_mut().zip(["x", "y", "z"]) {
                *angle = obj.get(key).map(|r| float(r)).transpose()?.unwrap_or(0.0);
            }
            RotationValue::EulerXyz(angles)
        } else {
            return Err(error("missing rotation value"));
        };
        let rescale = obj
            .get("rescale")
            .map(|r| boolean(r))
            .transpose()?
            .unwrap_or(false);
        Ok(Self {
            origin,
            value,
            rescale,
        })
    }
}
impl ItemTransform {
    fn decode(raw: &RawValue) -> Result<Self> {
        let obj = object(raw)?;
        let rotation = obj
            .get("rotation")
            .map(|r| vector(r))
            .transpose()?
            .unwrap_or([0.0; 3]);
        let translation = obj
            .get("translation")
            .map(|r| vector(r))
            .transpose()?
            .unwrap_or([0.0; 3])
            .map(|v| (v * 0.0625).clamp(-5.0, 5.0));
        let scale = obj
            .get("scale")
            .map(|r| vector(r))
            .transpose()?
            .unwrap_or([1.0; 3])
            .map(|v| v.clamp(-4.0, 4.0));
        Ok(Self {
            rotation,
            translation,
            scale,
        })
    }
}
impl ItemTransforms {
    fn decode(raw: &RawValue) -> Result<Self> {
        let obj = object(raw)?;
        let mut contexts = std::array::from_fn(|_| DisplayTransform::Inherit);
        for (transform, key) in contexts.iter_mut().zip(DISPLAY_CONTEXTS) {
            if let Some(raw) = obj.get(key) {
                *transform = DisplayTransform::Explicit(nullable(raw, ItemTransform::decode)?);
            }
        }
        for (left, right) in [(0, 1), (2, 3)] {
            if contexts[left] == DisplayTransform::Inherit {
                contexts[left] = contexts[right].clone();
            }
        }
        Ok(Self { contexts })
    }
}
