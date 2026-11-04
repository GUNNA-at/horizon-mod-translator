//! Meshes for NeoForge's OBJ model loader (`"loader": "neoforge:obj"`): Wavefront `.obj` geometry
//! whose `.mtl` materials name the texture each face is drawn with (`map_Kd`: one of the model's
//! texture slots, `#planks`, or a sprite). As NeoForge's `ObjModel` reads them: positions in block
//! units, a face of three corners repeats its last one (a quad), a face whose material is unknown
//! or unset draws nothing, and `flip_v` counts v from the top, as Blender writes it.
use crate::json::{Result, error};
use std::collections::BTreeMap;

/// One face, as `ObjModel.makeQuad` takes it before the model's transform.
#[derive(Clone, Debug, PartialEq)]
pub struct ObjFace {
    pub positions: [[f32; 3]; 4],
    /// Fractions of the face's sprite (`TextureAtlasSprite.getU(u)`), `flip_v` applied.
    pub uv: [[f32; 2]; 4],
    /// The first corner's normal: its `vn`, or the face's own when a corner has none.
    pub normal: [f32; 3],
    /// The material's `map_Kd`: a slot (`#planks`) or a sprite (`create:block/axis`).
    pub texture: String,
    /// `neoforge_TintIndex` (`forge_TintIndex`), -1 for none.
    pub tint_index: i32,
    /// `emissive_ambient`'s light from the material's `Ka`, 0 to 15.
    pub light: u8,
}

/// The faces of a mesh, in file order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ObjMesh {
    pub faces: Vec<ObjFace>,
}

/// A material: what NeoForge's `ObjMaterialLibrary.Material` keeps that a face is drawn with.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ObjMaterial {
    pub texture: Option<String>,
    pub tint_index: i32,
    pub ambient: [f32; 3],
}

/// An `.mtl` file's materials by name.
pub fn materials(mtl: &str) -> BTreeMap<String, ObjMaterial> {
    let mut out: BTreeMap<String, ObjMaterial> = BTreeMap::new();
    let mut current: Option<String> = None;
    for line in lines(mtl) {
        if line[0] == "newmtl" {
            let name = line[1..].join(" ");
            out.insert(name.clone(), ObjMaterial { tint_index: -1, ..Default::default() });
            current = Some(name);
            continue;
        }
        let Some(m) = current.as_ref().and_then(|n| out.get_mut(n)) else { continue };
        match line[0] {
            // Options before the file name are ignored, as NeoForge's reader does.
            "map_Kd" => m.texture = line.last().map(|s| (*s).to_owned()),
            "Ka" => {
                let v = vector(&line, 0.0);
                m.ambient = [v[0], v[1], v[2]];
            }
            "forge_TintIndex" | "neoforge_TintIndex" => m.tint_index = line.get(1).and_then(|s| s.parse().ok()).unwrap_or(-1),
            _ => {}
        }
    }
    out
}

/// The material libraries (`mtllib`) an `.obj` file names, as written: relative to the file's
/// folder, or `namespace:path`.
pub fn libraries(obj: &str) -> Vec<String> {
    lines(obj).filter(|l| l[0] == "mtllib" && l.len() > 1).map(|l| l[1].to_owned()).collect()
}

/// Parses an `.obj` mesh: `materials` holds its libraries' materials, `flip_v` and
/// `emissive_ambient` are the model's options.
pub fn parse(obj: &str, materials: &BTreeMap<String, ObjMaterial>, flip_v: bool, emissive_ambient: bool) -> Result<ObjMesh> {
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut uvs: Vec<[f32; 2]> = Vec::new();
    let mut normals: Vec<[f32; 3]> = Vec::new();
    let mut material: Option<&ObjMaterial> = None;
    let mut mesh = ObjMesh::default();
    for line in lines(obj) {
        match line[0] {
            "v" => {
                let v = vector(&line, 1.0);
                let w = if line.len() > 4 { v[3] } else { 1.0 };
                positions.push([v[0] / w, v[1] / w, v[2] / w]);
            }
            "vt" => {
                let v = vector(&line, 0.0);
                uvs.push([v[0], v[1]]);
            }
            "vn" => {
                let v = vector(&line, 0.0);
                normals.push([v[0], v[1], v[2]]);
            }
            "usemtl" => {
                let name = line[1..].join(" ");
                material = Some(materials.get(&name).ok_or_else(|| error("an OBJ material not in its library"))?);
            }
            "f" => {
                let corners = line[1..]
                    .iter()
                    .map(|c| corner(c, positions.len(), uvs.len(), normals.len()))
                    .collect::<Result<Vec<_>>>()?;
                if corners.len() < 3 {
                    return Err(error("an OBJ face with fewer than three corners"));
                }
                // A face with no material, or one without a texture, draws nothing.
                let Some(m) = material else { continue };
                let Some(texture) = m.texture.clone() else { continue };
                mesh.faces.push(face(&corners, &positions, &uvs, &normals, m, texture, flip_v, emissive_ambient)?);
            }
            _ => {}
        }
    }
    Ok(mesh)
}

/// Reads a model's mesh as NeoForge's `ObjLoader` does: the `.obj` at its location, with the
/// materials of its `mtl_override`, else of each `mtllib` the file names. A library is found
/// beside the `.obj` unless it names its namespace (`create:models/block/x.mtl`). `read` gives a
/// resource's text by identifier (`create:models/block/water_wheel/water_wheel.obj`).
pub fn load(geometry: &crate::model::ObjGeometry, read: impl Fn(&str) -> Option<String>) -> Result<ObjMesh> {
    let missing = |what: &str, id: &str| crate::json::DecodeError(format!("{what} {id} does not read"));
    let text = read(&geometry.model).ok_or_else(|| missing("OBJ model", &geometry.model))?;
    let (namespace, path) = geometry.model.split_once(':').unwrap_or(("minecraft", &geometry.model));
    let folder = path.rfind('/').map_or("", |i| &path[..=i]);
    let libraries = match &geometry.mtl_override {
        Some(lib) => vec![lib.clone()],
        None => libraries(&text),
    };
    let mut found = BTreeMap::new();
    for lib in libraries {
        let id = if lib.contains(':') { lib } else { format!("{namespace}:{folder}{lib}") };
        found.extend(materials(&read(&id).ok_or_else(|| missing("OBJ material library", &id))?));
    }
    parse(&text, &found, geometry.flip_v, geometry.emissive_ambient)
}

/// `ObjModel.DEFAULT_COORDS`: the corners' texture coordinates when the file gives none.
const DEFAULT_UV: [[f32; 2]; 4] = [[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]];

#[allow(clippy::too_many_arguments)]
fn face(
    corners: &[[Option<usize>; 3]],
    positions: &[[f32; 3]],
    uvs: &[[f32; 2]],
    normals: &[[f32; 3]],
    material: &ObjMaterial,
    texture: String,
    flip_v: bool,
    emissive_ambient: bool,
) -> Result<ObjFace> {
    let at = |i: usize| corners[i.min(corners.len() - 1)];
    let position = |i: usize| -> Result<[f32; 3]> {
        at(i)[0].and_then(|p| positions.get(p).copied()).ok_or_else(|| error("an OBJ face corner past the positions"))
    };
    let p: [[f32; 3]; 4] = [position(0)?, position(1)?, position(2)?, position(3)?];
    let uv = std::array::from_fn(|i| {
        let t = at(i)[1].and_then(|t| uvs.get(t).copied()).unwrap_or(DEFAULT_UV[i]);
        [t[0], if flip_v { 1.0 - t[1] } else { t[1] }]
    });
    // The face's own normal when a corner has none (`needsNormalRecalculation`), else the first's.
    let normal = match corners.iter().all(|c| c[2].is_some()) {
        true => at(0)[2].and_then(|n| normals.get(n).copied()).ok_or_else(|| error("an OBJ face corner past the normals"))?,
        false => {
            let a: [f32; 3] = std::array::from_fn(|i| p[1][i] - p[0][i]);
            let b: [f32; 3] = std::array::from_fn(|i| p[2][i] - p[0][i]);
            let n = [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
            let length = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            n.map(|v| v / length)
        }
    };
    let light = if emissive_ambient {
        let [r, g, b] = material.ambient;
        ((r + g + b) * 15.0 / 3.0).clamp(0.0, 15.0) as u8
    } else {
        0
    };
    Ok(ObjFace { positions: p, uv, normal, texture, tint_index: material.tint_index, light })
}

/// A face corner, `position/uv/normal` (1-based; negative counts back from the last), as indices.
fn corner(text: &str, positions: usize, uvs: usize, normals: usize) -> Result<[Option<usize>; 3]> {
    let mut out = [None; 3];
    for (slot, (part, len)) in text.split('/').zip([positions, uvs, normals]).enumerate() {
        if part.is_empty() {
            continue;
        }
        let n: i64 = part.parse().map_err(|_| error("an OBJ face corner that is not a number"))?;
        let index = if n < 0 { len as i64 + n } else { n - 1 };
        out[slot] = usize::try_from(index).ok();
    }
    if out[0].is_none() {
        return Err(error("an OBJ face corner with no position"));
    }
    Ok(out)
}

/// A vector's numbers after the keyword; missing ones are 0 (`w` is `default_w`).
fn vector(line: &[&str], default_w: f32) -> [f32; 4] {
    let n = |i: usize| line.get(i).and_then(|s| s.parse::<f32>().ok());
    [n(1).unwrap_or(0.0), n(2).unwrap_or(0.0), n(3).unwrap_or(0.0), n(4).unwrap_or(default_w)]
}

/// `ObjTokenizer.readAndSplitLine`: comments and blank lines skipped, a trailing `\` joins the next
/// line, words split on spaces and tabs.
fn lines(text: &str) -> impl Iterator<Item = Vec<&str>> {
    let mut raw = text.lines();
    std::iter::from_fn(move || {
        loop {
            let mut line = raw.next()?;
            if line.starts_with('#') {
                continue;
            }
            let mut words: Vec<&str> = Vec::new();
            loop {
                let continued = line.ends_with('\\');
                let part = if continued { &line[..line.len() - 1] } else { line };
                words.extend(part.split([' ', '\t']).filter(|w| !w.is_empty()));
                if !continued {
                    break;
                }
                match raw.next() {
                    Some(next) if !next.is_empty() && !next.starts_with('#') => line = next,
                    _ => break,
                }
            }
            if !words.is_empty() {
                return Some(words);
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::CuboidModel;

    const MTL: &str = "# Blender MTL\nnewmtl plank\nmap_Kd #planks\n\nnewmtl axis\nKa 1.0 1.0 1.0\nmap_Kd create:block/axis\nnewmtl bare\nKd 1 1 1\n";

    #[test]
    fn materials_name_their_texture_and_light() {
        let m = materials(MTL);
        assert_eq!(m["plank"].texture.as_deref(), Some("#planks"));
        assert_eq!(m["axis"].texture.as_deref(), Some("create:block/axis"));
        assert_eq!(m["axis"].ambient, [1.0, 1.0, 1.0]);
        assert_eq!(m["bare"].texture, None);
        assert_eq!(m["plank"].tint_index, -1);
    }

    #[test]
    fn a_triangle_repeats_its_last_corner_and_flip_v_counts_from_the_top() {
        let obj = "mtllib wheel.mtl\nv 0 0 0\nv 1 0 0\nv 1 1 0\nvt 0 0\nvt 1 0\nvt 1 0.25\nvn 0 0 1\nusemtl plank\nf 1/1/1 2/2/1 3/3/1\n";
        assert_eq!(libraries(obj), ["wheel.mtl"]);
        let mesh = parse(obj, &materials(MTL), true, true).unwrap();
        let f = &mesh.faces[0];
        assert_eq!(f.positions, [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [1.0, 1.0, 0.0]]);
        assert_eq!(f.uv, [[0.0, 1.0], [1.0, 1.0], [1.0, 0.75], [1.0, 0.75]]);
        assert_eq!((f.normal, f.texture.as_str(), f.light), ([0.0, 0.0, 1.0], "#planks", 0));
    }

    #[test]
    fn a_face_without_normals_takes_its_own_and_negative_indices_count_back() {
        // Counter-clockwise seen from -x: (0,0,1) x (0,1,1) points west.
        let obj = "v 0 0 0\nv 0 0 1\nv 0 1 1\nv 0 1 0\nusemtl axis\nf -4 -3 -2 -1\n";
        let f = &parse(obj, &materials(MTL), false, true).unwrap().faces[0];
        assert_eq!(f.normal, [-1.0, 0.0, 0.0]);
        assert_eq!(f.uv, DEFAULT_UV);
        // Ka 1 1 1 with emissive_ambient: full light.
        assert_eq!(f.light, 15);
    }

    #[test]
    fn a_model_bakes_its_mesh_with_its_parents_slots_turned_as_its_elements_would_be() {
        use crate::{
            bake::{BlockModelTransform, bake_block_model},
            model::Direction,
            resolve::ModelLibrary,
        };
        let mut wheel = CuboidModel::from_json(
            r#"{"parent":"create:block/wheel_textures","loader":"neoforge:obj","flip_v":true,"model":"create:models/block/wheel.obj"}"#,
        )
        .unwrap();
        let textures = CuboidModel::from_json(r#"{"textures":{"planks":"block/oak_planks"}}"#).unwrap();
        // A face flat on the east side, its normal east.
        let obj = "v 1 0 0\nv 1 1 0\nv 1 1 1\nv 1 0 1\nvn 1 0 0\nusemtl plank\nf 1//1 2//1 3//1 4//1\n";
        let geometry = wheel.obj.as_mut().unwrap();
        assert!(geometry.flip_v && geometry.automatic_culling);
        geometry.mesh = Some(std::sync::Arc::new(parse(obj, &materials(MTL), true, true).unwrap()));
        let library = ModelLibrary::new(BTreeMap::from([
            ("create:block/wheel".to_owned(), wheel),
            ("create:block/wheel_textures".to_owned(), textures),
        ]))
        .unwrap();
        let resolved = library.resolve("create:block/wheel").unwrap();
        let quads = bake_block_model(&resolved, BlockModelTransform::new([0, 0, 0], false)).unwrap();
        assert_eq!(quads.len(), 1);
        assert_eq!(quads[0].material.sprite, "minecraft:block/oak_planks");
        assert_eq!((quads[0].facing, quads[0].cull), (Direction::East, Some(Direction::East)));
        // A block state's quarter turn about y moves it to the side an element's east face goes to.
        let turn = BlockModelTransform::new([0, 1, 0], false);
        let turned = bake_block_model(&resolved, turn).unwrap();
        let side = turn.direction(Direction::East);
        assert_ne!(side, Direction::East);
        assert_eq!((turned[0].facing, turned[0].cull), (side, Some(side)));
    }

    #[test]
    fn libraries_are_found_beside_the_mesh_or_by_namespace_and_the_override_wins() {
        let files = BTreeMap::from([
            ("create:models/block/wheel/wheel.obj", "mtllib wheel.mtl\nv 0 0 0\nv 1 0 0\nv 1 1 0\nusemtl plank\nf 1 2 3\n"),
            ("create:models/block/wheel/wheel.mtl", "newmtl plank\nmap_Kd #planks\n"),
            ("create:models/other.mtl", "newmtl plank\nmap_Kd #logs\n"),
        ]);
        let read = |id: &str| files.get(id).map(|s| (*s).to_owned());
        let mut wheel = CuboidModel::from_json(r#"{"loader":"neoforge:obj","model":"create:models/block/wheel/wheel.obj"}"#)
            .unwrap()
            .obj
            .unwrap();
        assert_eq!(load(&wheel, read).unwrap().faces[0].texture, "#planks");
        wheel.mtl_override = Some("create:models/other.mtl".into());
        assert_eq!(load(&wheel, read).unwrap().faces[0].texture, "#logs");
        wheel.mtl_override = Some("gone.mtl".into());
        let error = load(&wheel, read).unwrap_err().to_string();
        assert!(error.contains("create:models/block/wheel/gone.mtl"), "{error}");
    }

    #[test]
    fn faces_without_a_material_or_texture_draw_nothing_and_an_unknown_one_is_an_error() {
        let obj = "v 0 0 0\nv 1 0 0\nv 1 1 0\nf 1 2 3\nusemtl bare\nf 1 2 3\n";
        assert!(parse(obj, &materials(MTL), false, true).unwrap().faces.is_empty());
        assert!(parse("usemtl nothing\n", &materials(MTL), false, true).is_err());
    }
}
