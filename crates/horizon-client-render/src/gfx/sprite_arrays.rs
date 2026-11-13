//! More block sprite arrays than the eight the shaders name.
//!
//! terrain.wgsl binds eight arrays of [`super::SPRITE_SPLIT`] layers: 16,384 block sprites, which
//! vanilla and most packs of mods never reach. A large pack does (365 mods: 23,177 block
//! textures). Its shaders are then given eight more arrays, written into their text here, so a
//! game with fewer sprites compiles the shaders exactly as the files have them and binds what it
//! always did. A vertex holds a sprite's layer in 15 bits: sixteen arrays of 2048 are all of them.

/// The arrays the shaders bind as they are written.
pub(crate) const BASE: u32 = 8;
/// The arrays they bind once [`widened`].
pub(crate) const MOST: u32 = 16;
/// terrain.wgsl's group 0 binding of the ninth array (the eighth's is 11, the emission
/// thresholds' 12).
pub(crate) const TERRAIN_FIRST: u32 = 13;
/// hdr_stats.wgsl's binding of the ninth array (its parameters are at 8).
pub(crate) const STATS_FIRST: u32 = 9;

/// How many arrays the shaders bind for a game whose sprites fill `arrays` of them.
pub(crate) fn bound(arrays: u32) -> u32 {
    if arrays > BASE { MOST } else { BASE }
}

/// The most arrays a device can be given: [`MOST`] where a shader stage may sample enough
/// textures for them beside the terrain's others (its light map, the thresholds, a shader
/// pack's own), else [`BASE`].
pub(crate) fn most(textures_per_stage: u32) -> u32 {
    if textures_per_stage >= MOST + 8 {
        MOST
    } else {
        BASE
    }
}

/// The group 0 binding of sprite array `i` in terrain.wgsl.
pub(crate) fn terrain_binding(i: u32) -> u32 {
    match i {
        0 => 1,
        i if i < BASE => 4 + i,
        i => TERRAIN_FIRST + i - BASE,
    }
}

/// The binding of sprite array `i` in hdr_stats.wgsl.
pub(crate) fn stats_binding(i: u32) -> u32 {
    if i < BASE { i } else { STATS_FIRST + i - BASE }
}

/// A shader text that binds [`BASE`] arrays (`sprites`, `sprites_1`... `sprites_7`, under any
/// prefix) made to bind [`MOST`]: eight more declarations after the eighth's, at bindings
/// `first`..., and every `switch` that ends with the eighth array as its `default` goes on
/// through the sixteenth.
pub(crate) fn widened(source: &str, first: u32) -> String {
    let mut out = String::with_capacity(source.len() + 4096);
    for line in source.split_inclusive('\n') {
        let code = line.trim_start();
        let named = |i: u32| line.replace("sprites_7", &format!("sprites_{i}"));
        if code.starts_with("@group(0) @binding(")
            && code.contains("sprites_7: texture_2d_array<f32>;")
        {
            out.push_str(line);
            let at = line.find("@binding(").map_or(0, |i| i + "@binding(".len());
            let end = at + line[at..].find(')').unwrap_or(0);
            for i in BASE..MOST {
                let more = named(i);
                out.push_str(&format!(
                    "{}{}{}",
                    &more[..at],
                    first + i - BASE,
                    &more[end..]
                ));
            }
        } else if code.starts_with("default:") && code.contains("sprites_7") {
            out.push_str(&line.replacen("default:", &format!("case {}u:", BASE - 1), 1));
            for i in BASE..MOST - 1 {
                out.push_str(&named(i).replacen("default:", &format!("case {i}u:"), 1));
            }
            out.push_str(&named(MOST - 1));
        } else {
            out.push_str(line);
        }
    }
    out
}

/// `source` as a game with `arrays` arrays of sprites compiles it: itself, or [`widened`].
pub(crate) fn source(source: &str, arrays: u32, first: u32) -> std::borrow::Cow<'_, str> {
    if bound(arrays) > BASE {
        widened(source, first).into()
    } else {
        source.into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn validate(text: &str) {
        let module = naga::front::wgsl::parse_str(text)
            .unwrap_or_else(|e| panic!("{}", e.emit_to_string(text)));
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .unwrap_or_else(|e| panic!("{e:?}"));
    }

    /// The terrain shader and the emission statistics with sixteen arrays are valid shaders that
    /// name every array once where it is bound and once in each place a sprite is sampled, and
    /// the shaders of a game with few sprites are the files' own text.
    #[test]
    fn the_shaders_take_sixteen_arrays() {
        // (The terrain as the game compiles it, behind the HDR light's prelude.)
        let terrain = &super::super::terrain_shader();
        assert_eq!(source(terrain, 8, TERRAIN_FIRST), terrain.as_str());
        let wide = widened(terrain, TERRAIN_FIRST);
        validate(&wide);
        for i in BASE..MOST {
            let declared = format!(
                "@group(0) @binding({}) var sprites_{i}: texture_2d_array<f32>;",
                terrain_binding(i)
            );
            assert_eq!(wide.matches(&declared).count(), 1, "{declared}");
            assert_eq!(
                wide.matches(&format!("(sprites_{i}, sprite_sampler,"))
                    .count(),
                2,
                "array {i} is sampled by both samplers"
            );
        }
        assert_eq!(
            wide.matches("default: { return textureSample").count(),
            terrain.matches("default: { return textureSample").count()
        );
        assert!(
            wide.contains("@group(0) @binding(12) var emission_thresholds"),
            "the thresholds keep their binding"
        );
        let stats = include_str!("../hdr_stats.wgsl");
        let wide = widened(stats, STATS_FIRST);
        validate(&wide);
        assert!(
            wide.contains("@group(0) @binding(8) var<uniform> params"),
            "the parameters keep their binding"
        );
        assert!(wide.contains("@group(0) @binding(16) var sprites_15: texture_2d_array<f32>;"));
        assert!(wide.contains("default: { return textureLoad(sprites_15, at, layer, 0); }"));
        // A shader pack's terrain: the same text under its prefix.
        let prefixed = "@group(0) @binding(11) var horizon_terrain_sprites_7: texture_2d_array<f32>;\n        default: { return textureSampleLevel(horizon_terrain_sprites_7, s, uv, l, lod); }\n";
        let wide = widened(prefixed, TERRAIN_FIRST);
        assert!(wide.contains(
            "@group(0) @binding(20) var horizon_terrain_sprites_15: texture_2d_array<f32>;"
        ));
        assert!(wide.contains(
            "case 7u: { return textureSampleLevel(horizon_terrain_sprites_7, s, uv, l, lod); }"
        ));
        assert!(wide.contains(
            "default: { return textureSampleLevel(horizon_terrain_sprites_15, s, uv, l, lod); }"
        ));
    }

    #[test]
    fn bindings_and_counts() {
        assert_eq!((bound(1), bound(8), bound(9), bound(16)), (8, 8, 16, 16));
        assert_eq!((most(16), most(23), most(24), most(96)), (8, 8, 16, 16));
        assert_eq!(
            (0..16).map(terrain_binding).collect::<Vec<_>>(),
            [1, 5, 6, 7, 8, 9, 10, 11, 13, 14, 15, 16, 17, 18, 19, 20]
        );
        assert_eq!(
            (0..16).map(stats_binding).collect::<Vec<_>>(),
            [0, 1, 2, 3, 4, 5, 6, 7, 9, 10, 11, 12, 13, 14, 15, 16]
        );
    }
}
