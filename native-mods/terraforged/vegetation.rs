/*
 * MIT License
 *
 * Copyright (c) 2021 TerraForged
 * Copyright (c) 2020 TerraForged
 *
 * Permission is hereby granted, free of charge, to any person obtaining a copy
 * of this software and associated documentation files (the "Software"), to deal
 * in the Software without restriction, including without limitation the rights
 * to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
 * copies of the Software, and to permit persons to whom the Software is
 * furnished to do so, subject to the following conditions:
 *
 * The above copyright notice and this permission notice shall be included in all
 * copies or substantial portions of the Software.
 *
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
 * IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
 * FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
 * AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
 * LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
 * OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
 * SOFTWARE.
 */

// TerraForged's bundled `worldgen/vegetation` resources as `VegetationConfig.CODEC` reads them (with
// `ViabilityCodec` and the viability specs), `BiomeVegetationManager.getConfig`'s lookup and `VegetationFeatures`'
// split of a biome's vegetal features (TerraForged 6dd607eb; Reimagined a24d450 has the same codec, lookup and
// split, plus `pale_garden_vegetation` among the tree keywords, which is followed here: Arthur's decision, so 26.3's
// pale oaks are placed as trees; the keyword is Reimagined's, MIT). The eight vegetation files
// are 6dd607eb's generated copies (`generated/resources/default/data/terraforged/worldgen/vegetation/`), bundled
// unchanged in `data/worldgen/vegetation/`; seven are byte-identical in TerraForged Reimagined a24d450
// (https://github.com/Romoslayer/TerraForged-Reimagined/tree/a24d4503064780790e975cd17495866ae13eef11, MIT,
// (c) 2020 TerraForged, (c) 2026 Romoslayer; see `LICENSE-TERRAFORGED-REIMAGINED`), while Reimagined's
// `trees_temperate.json` weighs its Height rule -0.5 instead of 6dd607eb's -1. Neither tree ships the `trees/*`
// biome tags the files select: the eight in `data/tags/worldgen/biome/trees/` are the ones TerraForged's official
// 0.3.1 alpha 2 jar ships (`default/data/terraforged/tags/worldgen/biome/trees/`, MIT), bundled unchanged.

use super::terraforged::Module;
use super::terraforged_biome_viability::Viability;
use super::terraforged_terrain_asset::{Data, Obj, codec_float, data_of, identifier};
use serde_json::value::RawValue;
use std::collections::{BTreeSet, HashMap};

/// A decoded `worldgen/vegetation` entry.
pub struct VegetationConfig {
    pub frequency: f32,
    pub jitter: f32,
    pub density: f32,
    /// The `biomes` tag's id (`namespace:path`).
    pub tag: String,
    /// The tag's members when it is one of the bundled `trees/*` tags; empty otherwise, as an unbound tag is.
    pub biomes: BTreeSet<String>,
    pub viability: Viability,
}

/// The bundled configs, in 26.3 registry order (`RegistryKey.entries` sorts by id: path, then namespace).
const CONFIGS: [(&str, &[u8]); 8] = [
    (
        "trees_copse",
        crate::terraforged_data!("worldgen/vegetation/trees_copse.json"),
    ),
    (
        "trees_hardy",
        crate::terraforged_data!("worldgen/vegetation/trees_hardy.json"),
    ),
    (
        "trees_hardy_slopes",
        crate::terraforged_data!("worldgen/vegetation/trees_hardy_slopes.json"),
    ),
    (
        "trees_patchy",
        crate::terraforged_data!("worldgen/vegetation/trees_patchy.json"),
    ),
    (
        "trees_rainforest",
        crate::terraforged_data!("worldgen/vegetation/trees_rainforest.json"),
    ),
    (
        "trees_sparse",
        crate::terraforged_data!("worldgen/vegetation/trees_sparse.json"),
    ),
    (
        "trees_sparse_rainforest",
        crate::terraforged_data!("worldgen/vegetation/trees_sparse_rainforest.json"),
    ),
    (
        "trees_temperate",
        crate::terraforged_data!("worldgen/vegetation/trees_temperate.json"),
    ),
];

const TAGS: [(&str, &[u8]); 8] = [
    (
        "terraforged:trees/copses",
        crate::terraforged_data!("tags/worldgen/biome/trees/copses.json"),
    ),
    (
        "terraforged:trees/hardy",
        crate::terraforged_data!("tags/worldgen/biome/trees/hardy.json"),
    ),
    (
        "terraforged:trees/hardy_slopes",
        crate::terraforged_data!("tags/worldgen/biome/trees/hardy_slopes.json"),
    ),
    (
        "terraforged:trees/patchy",
        crate::terraforged_data!("tags/worldgen/biome/trees/patchy.json"),
    ),
    (
        "terraforged:trees/rainforest",
        crate::terraforged_data!("tags/worldgen/biome/trees/rainforest.json"),
    ),
    (
        "terraforged:trees/sparse",
        crate::terraforged_data!("tags/worldgen/biome/trees/sparse.json"),
    ),
    (
        "terraforged:trees/sparse_rainforest",
        crate::terraforged_data!("tags/worldgen/biome/trees/sparse_rainforest.json"),
    ),
    (
        "terraforged:trees/temperate",
        crate::terraforged_data!("tags/worldgen/biome/trees/temperate.json"),
    ),
];

/// The bundled configs in registry order (`TerraForged.VEGETATIONS.entries`).
///
/// The tags' members are listed as the files name them. 26.3 drops a whole tag that names a biome the registry
/// lacks (`TagLoader`: "missing following references"), and `trees/patchy` names `terraforged:oak_forest`, which
/// only a bundled biome provides: [`bind_tags`] applies that rule to a world's registry.
pub fn current_vegetation() -> Result<Vec<VegetationConfig>, String> {
    CONFIGS
        .iter()
        .map(|(name, json)| {
            decode_config(text(json)).map_err(|error| format!("vegetation {name}: {error}"))
        })
        .collect()
}

/// Binds `configs`' tags to a world's biome registry as 26.3's `TagLoader` loads a tag: a tag naming a biome
/// `is_registered` does not know fails to load ("missing following references") and is dropped whole, so its
/// config holds no biome (as an unbound `TagKey` holds none). `trees/patchy` names `terraforged:oak_forest`, which
/// only a bundled TerraForged biome provides.
pub fn bind_tags(configs: &mut [VegetationConfig], is_registered: impl Fn(&str) -> bool) {
    for config in configs {
        if !config.biomes.iter().all(|biome| is_registered(biome)) {
            config.biomes.clear();
        }
    }
}

/// `BiomeVegetationManager.getConfig`: the first config whose tag holds the biome (`None` for Java's
/// `VegetationConfig.NONE`).
///
/// `biome` is a full `namespace:path` id. Java caches the answer per biome (`computeIfAbsent`); the result does
/// not change, so callers may cache it too.
pub fn config_for<'a>(
    configs: &'a [VegetationConfig],
    biome: &str,
) -> Option<&'a VegetationConfig> {
    configs.iter().find(|config| config.biomes.contains(biome))
}

/// `VegetationConfig.CODEC` over Minecraft's strict JSON (a field set to `null` counts as absent, as `JsonOps`
/// reads it; unknown fields are ignored):
///
/// - `frequency`, `jitter`, `density` (`Codec.FLOAT`, optional, default 1): a JSON number read with
///   `Float.parseFloat` on its text; anything else present is an error.
/// - `biomes` (`TagKey.hashedCodec`): a string `#` + an identifier (`Identifier.read`: no namespace means
///   `minecraft`). A tag that is not bundled decodes, with no members.
/// - `viability` (`ViabilityCodec.CODEC`, required): a Cereal graph; see [`decode_viability`].
pub fn decode_config(json: &str) -> Result<VegetationConfig, String> {
    let fields: HashMap<String, &RawValue> =
        serde_json::from_str(json).map_err(|error| error.to_string())?;
    let field = |name: &str| {
        fields
            .get(name)
            .map(|raw| raw.get())
            .filter(|raw| *raw != "null")
    };
    let number = |name: &str| {
        field(name).map_or(Ok(1.0), |raw| {
            codec_float(raw).ok_or_else(|| format!("{name} must be a number"))
        })
    };
    let biomes = field("biomes").ok_or("missing field \"biomes\"")?;
    let biomes = serde_json::from_str::<String>(biomes).map_err(|_| "biomes must be a string")?;
    let tag = biomes
        .strip_prefix('#')
        .ok_or_else(|| format!("biomes {biomes:?} is not a tag"))?;
    let tag = identifier(tag).ok_or_else(|| format!("invalid tag {tag:?}"))?;
    let viability = field("viability").ok_or("missing field \"viability\"")?;
    let viability = match data_of(viability).map_err(|error| error.to_string())? {
        Data::Obj(object) => decode_viability(&object)?,
        _ => return Err("viability must be an object".into()),
    };
    Ok(VegetationConfig {
        frequency: number("frequency")?,
        jitter: number("jitter")?,
        density: number("density")?,
        biomes: tag_members(&tag)?,
        tag,
        viability,
    })
}

/// A bundled tag's `values` (plain biome ids only: the bundled tags hold no tag references or optional entries).
fn tag_members(tag: &str) -> Result<BTreeSet<String>, String> {
    let Some((_, json)) = TAGS.iter().find(|(name, _)| *name == tag) else {
        return Ok(BTreeSet::new());
    };
    let value: serde_json::Value =
        serde_json::from_str(text(json)).map_err(|error| error.to_string())?;
    value["values"]
        .as_array()
        .ok_or("tag values must be an array")?
        .iter()
        .map(|entry| {
            entry
                .as_str()
                .filter(|id| !id.starts_with('#'))
                .and_then(identifier)
                .ok_or_else(|| format!("unsupported tag entry {entry}"))
        })
        .collect()
}

/// `ViabilityCodec`: `Cereal.deserialize(data, Viability.class)` over the specs `ViabilityCodec` registers, with
/// Cereal's rules (as `terrain_asset.rs` reads noise): a missing scalar takes its spec's default and a non-number
/// reads as 0 (`asFloat`); `rules` and `amounts` that are missing or not a list are empty, `rules` skips its
/// non-object entries and `amounts` reads every entry with `asFloat`; `noise` must be a noise graph object.
/// Defaults: `BiomeEdgeViability` distance 1; `Height` min 0, mid 0.5, max 1; `Saturation` 0..1; `Slope` normalize
/// 1, max 1; `Sum` initial 1.
///
/// Differences: a spec name that is not a viability's is an error, where Cereal, for a name no spec registers,
/// tries every viability spec whose default fields the object has with the same value types, in hash order
/// (ambiguous for, e.g., a nameless `min`/`mid`/`max` object, which `Height` and `Saturation` both fit); and a `Sum`
/// with fewer `amounts` than `rules` is an error here, where Java decodes it and throws on its first sample.
pub(super) fn decode_viability(object: &Obj) -> Result<Viability, String> {
    let o = object;
    Ok(match o.kind.as_str() {
        "BiomeEdgeViability" => Viability::BiomeEdge {
            distance: o.float("distance", 1.0),
        },
        "Height" => Viability::Height {
            min_offset: o.float("min", 0.0),
            mid_offset: o.float("mid", 0.5),
            max_offset: o.float("max", 1.0),
        },
        "Multiply" => Viability::Multiply(rules(o)?),
        "Noise" => Viability::Noise(noise(o)?),
        "Saturation" => Viability::Saturation {
            min: o.float("min", 0.0),
            max: o.float("max", 1.0),
        },
        "Slope" => Viability::Slope {
            normalize: o.float("normalize", 1.0),
            max: o.float("max", 1.0),
        },
        "Sum" => {
            let rules = rules(o)?;
            let amounts: Vec<f32> = match o.scalar("amounts") {
                Some(Data::List(list)) => list.iter().map(Data::as_float).collect(),
                _ => Vec::new(),
            };
            if amounts.len() < rules.len() {
                return Err(format!(
                    "Sum has {} rules but {} amounts",
                    rules.len(),
                    amounts.len()
                ));
            }
            Viability::Sum {
                initial: o.float("initial", 1.0),
                rules,
                amounts,
            }
        }
        other => return Err(format!("no viability spec {other:?}")),
    })
}

fn rules(object: &Obj) -> Result<Vec<Viability>, String> {
    match object.scalar("rules") {
        Some(Data::List(list)) => list
            .iter()
            .filter_map(|value| match value {
                Data::Obj(rule) => Some(decode_viability(rule)),
                _ => None,
            })
            .collect(),
        _ => Ok(Vec::new()),
    }
}

fn noise(object: &Obj) -> Result<Module, String> {
    object.child("noise").map_err(|error| error.to_string())
}

fn text(bytes: &'static [u8]) -> &'static str {
    std::str::from_utf8(bytes).expect("TerraForged vegetation JSON is UTF-8")
}

/// `VegetationFeatures.create`: a biome's `VEGETAL_DECORATION` features split by the paths of their registry ids
/// into trees, grass and the rest, as `(feature, unwrap)` where `unwrap` asks for the tree's placement without its
/// count, square and biome modifiers (custom configs only). Java puts a feature with no registry id (an inline one)
/// in `other`; the caller adds those there itself.
pub struct VegetationFeatures {
    pub trees: Vec<(String, bool)>,
    pub grass: Vec<String>,
    pub other: Vec<String>,
}

/// 6dd607eb's tree keywords, plus TerraForged Reimagined a24d450's `pale_garden_vegetation` (MIT, (c) 2026
/// Romoslayer): 26.3's pale garden names its pale oaks that way, which no 6dd607eb keyword matches, so without it
/// they would fall in `other` (placed once at the chunk's corner, not as trees). A departure from 6dd607eb, by
/// Arthur's decision.
const TREE_KEYWORDS: [&str; 7] = [
    "tree",
    "spruce",
    "oak",
    "birch",
    "pine",
    "dark_forest_vegetation",
    "pale_garden_vegetation",
];
const GRASS_KEYWORDS: [&str; 1] = ["grass"];

impl VegetationFeatures {
    /// `vegetal`: the registry ids of the biome's `VEGETAL_DECORATION` features, in order; `custom`: the biome has
    /// a vegetation config (`config != VegetationConfig.NONE`).
    pub fn create<'a>(vegetal: impl IntoIterator<Item = &'a str>, custom: bool) -> Self {
        let mut features = Self {
            trees: Vec::new(),
            grass: Vec::new(),
            other: Vec::new(),
        };
        for id in vegetal {
            let path = id.split_once(':').map_or(id, |(_, path)| path);
            if TREE_KEYWORDS.iter().any(|keyword| path.contains(keyword)) {
                features.trees.push((id.to_owned(), custom));
            } else if GRASS_KEYWORDS.iter().any(|keyword| path.contains(keyword)) {
                features.grass.push(id.to_owned());
            } else {
                features.other.push(id.to_owned());
            }
        }
        features
    }
}

#[cfg(test)]
mod tests {
    use super::super::terraforged::{clamp, hash_2d};
    use super::super::terraforged_biome_registry::identifier_order;
    use super::super::terraforged_terrain_asset::tests::{bits, sample_hash};
    use super::*;

    /// Decode edge cases (`oracles/vegetation/cases.txt`): name, tab, document.
    const CASES: &str = r##"# name<TAB>json: VegetationConfig.CODEC edge cases (top-level DFU fields, then Cereal viability specs).
minimal	{"biomes":"#terraforged:trees/copses","viability":{"type":"Saturation"}}
numbers	{"biomes":"#terraforged:trees/copses","frequency":0.25,"jitter":-0.0,"density":3,"viability":{"type":"Saturation"}}
freq_double_rounding	{"biomes":"#terraforged:trees/copses","frequency":1.0000000596046447753906251,"viability":{"type":"Saturation"}}
freq_exponent	{"biomes":"#terraforged:trees/copses","frequency":2.5e-3,"viability":{"type":"Saturation"}}
freq_huge	{"biomes":"#terraforged:trees/copses","frequency":1e40,"viability":{"type":"Saturation"}}
freq_long	{"biomes":"#terraforged:trees/copses","frequency":9223372036854775807,"viability":{"type":"Saturation"}}
freq_null	{"biomes":"#terraforged:trees/copses","frequency":null,"viability":{"type":"Saturation"}}
freq_bool	{"biomes":"#terraforged:trees/copses","frequency":true,"viability":{"type":"Saturation"}}
freq_string	{"biomes":"#terraforged:trees/copses","frequency":"0.5","viability":{"type":"Saturation"}}
freq_object	{"biomes":"#terraforged:trees/copses","frequency":{},"viability":{"type":"Saturation"}}
biomes_missing	{"viability":{"type":"Saturation"}}
biomes_no_hash	{"biomes":"terraforged:trees/copses","viability":{"type":"Saturation"}}
biomes_no_namespace	{"biomes":"#trees/copses","viability":{"type":"Saturation"}}
biomes_leading_colon	{"biomes":"#:trees/copses","viability":{"type":"Saturation"}}
biomes_invalid	{"biomes":"#Trees/copses","viability":{"type":"Saturation"}}
biomes_unknown	{"biomes":"#terraforged:trees/none","viability":{"type":"Saturation"}}
biomes_number	{"biomes":5,"viability":{"type":"Saturation"}}
biomes_list	{"biomes":["#terraforged:trees/copses"],"viability":{"type":"Saturation"}}
viability_missing	{"biomes":"#terraforged:trees/copses"}
viability_null	{"biomes":"#terraforged:trees/copses","viability":null}
viability_number	{"biomes":"#terraforged:trees/copses","viability":3}
viability_untyped_unmatched	{"biomes":"#terraforged:trees/copses","viability":{"foo":1}}
viability_unknown	{"biomes":"#terraforged:trees/copses","viability":{"type":"Nope","min":0,"max":1,"mid":1,"distance":1,"normalize":1}}
viability_noise_spec	{"biomes":"#terraforged:trees/copses","viability":{"type":"Const","value":1}}
viability_type_number	{"biomes":"#terraforged:trees/copses","viability":{"type":5}}
edge_default	{"biomes":"#terraforged:trees/copses","viability":{"type":"BiomeEdgeViability"}}
edge_value	{"biomes":"#terraforged:trees/copses","viability":{"type":"BiomeEdgeViability","distance":0.65}}
edge_short_name	{"biomes":"#terraforged:trees/copses","viability":{"type":"BiomeEdge","distance":0.65}}
height_default	{"biomes":"#terraforged:trees/copses","viability":{"type":"Height"}}
height_values	{"biomes":"#terraforged:trees/copses","viability":{"type":"Height","min":-100,"mid":35,"max":150}}
height_string	{"biomes":"#terraforged:trees/copses","viability":{"type":"Height","min":"5","mid":true,"max":[1]}}
saturation_values	{"biomes":"#terraforged:trees/copses","viability":{"type":"Saturation","min":0.7,"max":1}}
slope_default	{"biomes":"#terraforged:trees/copses","viability":{"type":"Slope"}}
slope_values	{"biomes":"#terraforged:trees/copses","viability":{"type":"Slope","normalize":65,"max":0.55}}
slope_double	{"biomes":"#terraforged:trees/copses","viability":{"type":"Slope","normalize":0.6000000002749595,"max":-0.799999999450081}}
mult_empty	{"biomes":"#terraforged:trees/copses","viability":{"type":"Multiply"}}
mult_rules	{"biomes":"#terraforged:trees/copses","viability":{"type":"Multiply","rules":[{"type":"Slope"},{"type":"Height","min":1}]}}
mult_rules_not_list	{"biomes":"#terraforged:trees/copses","viability":{"type":"Multiply","rules":{"type":"Slope"}}}
mult_rules_skip_scalars	{"biomes":"#terraforged:trees/copses","viability":{"type":"Multiply","rules":[1,"x",{"type":"Slope"},[{"type":"Height"}]]}}
mult_rule_error	{"biomes":"#terraforged:trees/copses","viability":{"type":"Multiply","rules":[{"type":"Nope"}]}}
mult_rule_noise_spec	{"biomes":"#terraforged:trees/copses","viability":{"type":"Multiply","rules":[{"type":"Const"}]}}
sum_default	{"biomes":"#terraforged:trees/copses","viability":{"type":"Sum"}}
sum_values	{"biomes":"#terraforged:trees/copses","viability":{"type":"Sum","initial":0.5,"amounts":[0.2,-1],"rules":[{"type":"Saturation"},{"type":"Slope"}]}}
sum_amount_scalars	{"biomes":"#terraforged:trees/copses","viability":{"type":"Sum","amounts":[0.2,"x",true,{},[1],-0.0],"rules":[]}}
sum_amounts_not_list	{"biomes":"#terraforged:trees/copses","viability":{"type":"Sum","amounts":0.5,"rules":[{"type":"Slope"}]}}
sum_fewer_amounts	{"biomes":"#terraforged:trees/copses","viability":{"type":"Sum","amounts":[0.5],"rules":[{"type":"Slope"},{"type":"Slope"}]}}
sum_more_amounts	{"biomes":"#terraforged:trees/copses","viability":{"type":"Sum","amounts":[0.5,1,2],"rules":[{"type":"Slope"}]}}
sum_nested	{"biomes":"#terraforged:trees/copses","viability":{"type":"Sum","amounts":[1],"rules":[{"type":"Multiply","rules":[{"type":"Sum","initial":2}]}]}}
noise_missing	{"biomes":"#terraforged:trees/copses","viability":{"type":"Noise"}}
noise_not_object	{"biomes":"#terraforged:trees/copses","viability":{"type":"Noise","noise":1}}
noise_const	{"biomes":"#terraforged:trees/copses","viability":{"type":"Noise","noise":{"type":"Const","value":0.25}}}
noise_simplex	{"biomes":"#terraforged:trees/copses","viability":{"type":"Noise","noise":{"type":"Map","min":{"type":"Const","value":0},"max":{"type":"Const","value":1},"source":{"type":"Clamp","min":{"type":"Const","value":0.4},"max":{"type":"Const","value":0.6},"source":{"type":"Simplex","seed":5,"frequency":0.008333334,"octaves":2,"interpolation":"CURVE3"}}}}}
noise_unknown	{"biomes":"#terraforged:trees/copses","viability":{"type":"Noise","noise":{"type":"Nope"}}}"##;

    /// `oracles/vegetation/run.sh`'s `out.txt`: TerraForged 6dd607eb's own `VegetationConfig.CODEC` (Cereal,
    /// Noise2D 2cb4843, Minecraft 26.3's DFU and strict JSON) over the bundled files in registry order (`f` lines)
    /// and over `CASES`. Floats are hex bits; a Noise rule shows its bounds and `sample_hash`.
    const VEGETATION_ORACLE: &str = r##"f trees_copse ok 3e4ccccd 3f4ccccd 3f19999a terraforged:trees/copses sum(00000000,[3e4ccccd,bf800000,bf000000,3f800000],[saturation(3f333333,3f800000),height(c2c80000,420c0000,43160000),slope(42820000,3f0ccccd),noise(00000000,3f800000,5458a430028305b9)])
f trees_hardy ok 3e6147ae 3f4ccccd 3f333333 terraforged:trees/hardy sum(3f000000,[3e4ccccd,bf800000,bf4ccccd,bf4ccccd,becccccd],[saturation(3f59999a,3f800000),height(c2c80000,42200000,433e0000),slope(425c0000,3f266666),edge(3f266666),noise(00000000,3f800000,59320cfc83e3ac8e)])
f trees_hardy_slopes ok 3e4ccccd 3f4ccccd 3f333333 terraforged:trees/hardy_slopes sum(3e4ccccd,[3e4ccccd,bf800000,3f800000,bf4ccccd,bf000000],[saturation(3f4ccccd,3f800000),height(c2c80000,42200000,43160000),slope(42700000,3f000000),edge(3f266666),noise(00000000,3f800000,4fd8bec3f5e06562)])
f trees_patchy ok 3e4ccccd 3f400000 3f000000 terraforged:trees/patchy sum(3f266666,[3e4ccccd,bf800000,bf800000,bf400000,bee66666],[saturation(3f666666,3f800000),height(c2c80000,42200000,43250000),slope(42700000,3f266666),edge(3f4ccccd),noise(00000000,3f800000,9c4e0c52bd140864)])
f trees_rainforest ok 3eb33333 3f400000 3f333333 terraforged:trees/rainforest sum(3ee66666,[3e800000,bf800000,bf000000,bf4ccccd,becccccd],[saturation(3f333333,3f800000),height(c2c80000,42700000,43340000),slope(425c0000,3f266666),edge(3f333333),noise(00000000,3f800000,66bfc1067f683a47)])
f trees_sparse ok 3e19999a 3f400000 3eb33333 terraforged:trees/sparse sum(00000000,[3ecccccd,bf800000,bf800000,3f800000],[saturation(3f733333,3f800000),height(c2c80000,42480000,432f0000),slope(42820000,3f19999a),noise(00000000,3f800000,c0dc458c554364f3)])
f trees_sparse_rainforest ok 3e19999a 3f4ccccd 3ee66666 terraforged:trees/sparse_rainforest sum(00000000,[3e4ccccd,bf800000,bf000000,3f000000],[saturation(3f266666,3f800000),height(c2c80000,41a00000,43160000),slope(42820000,3f400000),noise(00000000,3f800000,bbc8bf5ddd673e62)])
f trees_temperate ok 3e4ccccd 3f4ccccd 3f19999a terraforged:trees/temperate sum(3f333333,[3e800000,bf800000,bf19999a,bf4ccccd,bf000000],[saturation(3f733333,3f800000),height(c2c80000,42340000,43160000),slope(425c0000,3f266666),edge(3f333333),noise(00000000,3f800000,417842691c8af4f4)])
case minimal ok 3f800000 3f800000 3f800000 terraforged:trees/copses saturation(00000000,3f800000)
case numbers ok 3e800000 80000000 40400000 terraforged:trees/copses saturation(00000000,3f800000)
case freq_double_rounding ok 3f800001 3f800000 3f800000 terraforged:trees/copses saturation(00000000,3f800000)
case freq_exponent ok 3b23d70a 3f800000 3f800000 terraforged:trees/copses saturation(00000000,3f800000)
case freq_huge ok 7f800000 3f800000 3f800000 terraforged:trees/copses saturation(00000000,3f800000)
case freq_long ok 5f000000 3f800000 3f800000 terraforged:trees/copses saturation(00000000,3f800000)
case freq_null ok 3f800000 3f800000 3f800000 terraforged:trees/copses saturation(00000000,3f800000)
case freq_bool err
case freq_string err
case freq_object err
case biomes_missing err
case biomes_no_hash err
case biomes_no_namespace ok 3f800000 3f800000 3f800000 minecraft:trees/copses saturation(00000000,3f800000)
case biomes_leading_colon ok 3f800000 3f800000 3f800000 minecraft:trees/copses saturation(00000000,3f800000)
case biomes_invalid err
case biomes_unknown ok 3f800000 3f800000 3f800000 terraforged:trees/none saturation(00000000,3f800000)
case biomes_number err
case biomes_list err
case viability_missing err
case viability_null err
case viability_number err
case viability_untyped_unmatched err
case viability_unknown err
case viability_noise_spec err
case viability_type_number err
case edge_default ok 3f800000 3f800000 3f800000 terraforged:trees/copses edge(3f800000)
case edge_value ok 3f800000 3f800000 3f800000 terraforged:trees/copses edge(3f266666)
case edge_short_name err
case height_default ok 3f800000 3f800000 3f800000 terraforged:trees/copses height(00000000,3f000000,3f800000)
case height_values ok 3f800000 3f800000 3f800000 terraforged:trees/copses height(c2c80000,420c0000,43160000)
case height_string ok 3f800000 3f800000 3f800000 terraforged:trees/copses height(00000000,00000000,00000000)
case saturation_values ok 3f800000 3f800000 3f800000 terraforged:trees/copses saturation(3f333333,3f800000)
case slope_default ok 3f800000 3f800000 3f800000 terraforged:trees/copses slope(3f800000,3f800000)
case slope_values ok 3f800000 3f800000 3f800000 terraforged:trees/copses slope(42820000,3f0ccccd)
case slope_double ok 3f800000 3f800000 3f800000 terraforged:trees/copses slope(3f19999a,bf4ccccd)
case mult_empty ok 3f800000 3f800000 3f800000 terraforged:trees/copses mult[]
case mult_rules ok 3f800000 3f800000 3f800000 terraforged:trees/copses mult[slope(3f800000,3f800000),height(3f800000,3f000000,3f800000)]
case mult_rules_not_list ok 3f800000 3f800000 3f800000 terraforged:trees/copses mult[]
case mult_rules_skip_scalars ok 3f800000 3f800000 3f800000 terraforged:trees/copses mult[slope(3f800000,3f800000)]
case mult_rule_error err
case mult_rule_noise_spec err
case sum_default ok 3f800000 3f800000 3f800000 terraforged:trees/copses sum(3f800000,[],[])
case sum_values ok 3f800000 3f800000 3f800000 terraforged:trees/copses sum(3f000000,[3e4ccccd,bf800000],[saturation(00000000,3f800000),slope(3f800000,3f800000)])
case sum_amount_scalars ok 3f800000 3f800000 3f800000 terraforged:trees/copses sum(3f800000,[3e4ccccd,00000000,00000000,00000000,00000000,00000000],[])
case sum_amounts_not_list ok 3f800000 3f800000 3f800000 terraforged:trees/copses sum(3f800000,[],[slope(3f800000,3f800000)])
case sum_fewer_amounts ok 3f800000 3f800000 3f800000 terraforged:trees/copses sum(3f800000,[3f000000],[slope(3f800000,3f800000),slope(3f800000,3f800000)])
case sum_more_amounts ok 3f800000 3f800000 3f800000 terraforged:trees/copses sum(3f800000,[3f000000,3f800000,40000000],[slope(3f800000,3f800000)])
case sum_nested ok 3f800000 3f800000 3f800000 terraforged:trees/copses sum(3f800000,[3f800000],[mult[sum(40000000,[],[])]])
case noise_missing err
case noise_not_object err
case noise_const ok 3f800000 3f800000 3f800000 terraforged:trees/copses noise(3e800000,3e800000,3d608a822f1743bd)
case noise_simplex ok 3f800000 3f800000 3f800000 terraforged:trees/copses noise(00000000,3f800000,b26ecdfbd3fdf592)
case noise_unknown err
"##;

    /// Cases Java decodes and the port rejects (see `decode_viability`).
    const PORT_REJECTS: [&str; 2] = ["sum_amounts_not_list", "sum_fewer_amounts"];

    fn hex(value: f32) -> String {
        format!("{:08x}", bits(value))
    }

    fn list(rules: &[Viability]) -> String {
        let rules: Vec<String> = rules.iter().map(describe).collect();
        format!("[{}]", rules.join(","))
    }

    fn describe(viability: &Viability) -> String {
        match viability {
            Viability::None => "none".into(),
            Viability::BiomeEdge { distance } => format!("edge({})", hex(*distance)),
            Viability::Height {
                min_offset,
                mid_offset,
                max_offset,
            } => format!(
                "height({},{},{})",
                hex(*min_offset),
                hex(*mid_offset),
                hex(*max_offset)
            ),
            Viability::Multiply(rules) => format!("mult{}", list(rules)),
            Viability::Noise(noise) => format!(
                "noise({},{},{:016x})",
                hex(noise.min_value()),
                hex(noise.max_value()),
                sample_hash(noise)
            ),
            Viability::Saturation { min, max } => {
                format!("saturation({},{})", hex(*min), hex(*max))
            }
            Viability::Slope { normalize, max } => {
                format!("slope({},{})", hex(*normalize), hex(*max))
            }
            Viability::Sum {
                initial,
                rules,
                amounts,
            } => {
                let amounts: Vec<String> = amounts.iter().map(|amount| hex(*amount)).collect();
                format!(
                    "sum({},[{}],{})",
                    hex(*initial),
                    amounts.join(","),
                    list(rules)
                )
            }
        }
    }

    fn line(config: Result<&VegetationConfig, &String>) -> String {
        match config {
            Ok(config) => format!(
                "ok {} {} {} {} {}",
                hex(config.frequency),
                hex(config.jitter),
                hex(config.density),
                config.tag,
                describe(&config.viability)
            ),
            Err(_) => "err".into(),
        }
    }

    fn oracle(prefix: &str) -> Vec<&'static str> {
        VEGETATION_ORACLE
            .lines()
            .filter(|line| line.starts_with(prefix))
            .collect()
    }

    #[test]
    fn bundled_configs_decode_like_6dd607eb_java_in_registry_order() {
        let configs = current_vegetation().unwrap();
        let lines: Vec<String> = CONFIGS
            .iter()
            .zip(&configs)
            .map(|((name, _), config)| format!("f {name} {}", line(Ok(config))))
            .collect();
        assert_eq!(lines, oracle("f "));
    }

    #[test]
    fn edge_cases_decode_like_6dd607eb_java() {
        let java = oracle("case ");
        let cases: Vec<&str> = CASES
            .lines()
            .filter(|row| !row.is_empty() && !row.starts_with('#'))
            .collect();
        assert_eq!(cases.len(), java.len());
        for (row, java) in cases.iter().zip(java) {
            let (name, json) = row.split_once('\t').unwrap();
            let rust = format!("case {name} {}", line(decode_config(json).as_ref()));
            if PORT_REJECTS.contains(&name) {
                assert_eq!(rust, format!("case {name} err"));
                assert!(java.starts_with(&format!("case {name} ok ")), "{java}");
            } else {
                assert_eq!(rust, java);
            }
        }
    }

    #[test]
    fn configs_can_be_shared_across_generation_threads() {
        fn shared<T: Send + Sync>() {}
        shared::<VegetationConfig>();
        shared::<Vec<VegetationConfig>>();
    }

    #[test]
    fn configs_and_tags_follow_the_26_3_registry_order() {
        let ids: Vec<String> = CONFIGS
            .iter()
            .map(|(name, _)| format!("terraforged:{name}"))
            .collect();
        let mut sorted = ids.clone();
        sorted.sort_by(|left, right| identifier_order(left, right));
        assert_eq!(ids, sorted);
        // Each config selects one bundled tag, so every tag is used once.
        let configs = current_vegetation().unwrap();
        let mut tags: Vec<&str> = configs.iter().map(|config| config.tag.as_str()).collect();
        tags.sort_unstable();
        let bundled: Vec<&str> = TAGS.iter().map(|(name, _)| *name).collect();
        assert_eq!(tags, bundled);
        assert!(configs.iter().all(|config| !config.biomes.is_empty()));
    }

    #[test]
    fn biomes_take_the_first_config_whose_tag_holds_them() {
        let configs = current_vegetation().unwrap();
        let tag = |biome: &str| config_for(&configs, biome).map(|config| config.tag.as_str());
        assert_eq!(
            tag("minecraft:birch_forest"),
            Some("terraforged:trees/temperate")
        );
        // Taiga is in both trees/hardy and trees/hardy_slopes: trees_hardy comes first.
        assert_eq!(tag("minecraft:taiga"), Some("terraforged:trees/hardy"));
        assert_eq!(
            tag("minecraft:dark_forest"),
            Some("terraforged:trees/hardy")
        );
        assert_eq!(tag("minecraft:plains"), Some("terraforged:trees/copses"));
        assert_eq!(
            tag("terraforged:oak_forest"),
            Some("terraforged:trees/patchy")
        );
        assert_eq!(
            tag("minecraft:sparse_jungle"),
            Some("terraforged:trees/sparse_rainforest")
        );
        assert_eq!(tag("minecraft:desert"), None);
        assert_eq!(tag("minecraft:pale_garden"), None);
    }

    /// 26.3's `TagLoader` drops a tag naming an unregistered biome whole: without `terraforged:oak_forest`,
    /// `trees/patchy` loses `minecraft:wooded_badlands` too, which then has no config (it is in no other tag).
    #[test]
    fn tags_naming_a_missing_biome_are_dropped_whole() {
        let mut configs = current_vegetation().unwrap();
        bind_tags(&mut configs, |biome| biome.starts_with("minecraft:"));
        let patchy = configs
            .iter()
            .find(|config| config.tag == "terraforged:trees/patchy")
            .unwrap();
        assert!(patchy.biomes.is_empty());
        assert!(config_for(&configs, "minecraft:wooded_badlands").is_none());
        assert_eq!(
            config_for(&configs, "minecraft:birch_forest").map(|config| config.tag.as_str()),
            Some("terraforged:trees/temperate")
        );
        let dropped = configs
            .iter()
            .filter(|config| config.biomes.is_empty())
            .count();
        assert_eq!(dropped, 1);
        // With the biome registered, every tag binds.
        let mut configs = current_vegetation().unwrap();
        bind_tags(&mut configs, |_| true);
        assert!(configs.iter().all(|config| !config.biomes.is_empty()));
    }

    #[test]
    fn vegetal_features_split_like_terraforged() {
        let features = VegetationFeatures::create(
            [
                "minecraft:trees_birch_and_oak_leaf_litter",
                "minecraft:patch_grass_forest",
                "minecraft:flower_default",
                "minecraft:dark_forest_vegetation",
                "minecraft:pale_garden_vegetation",
                "patch_tall_grass",
                "minecraft:mushroom_island_vegetation",
                "oak:flower",
            ],
            true,
        );
        assert_eq!(
            features.trees,
            [
                ("minecraft:trees_birch_and_oak_leaf_litter".to_owned(), true),
                ("minecraft:dark_forest_vegetation".to_owned(), true),
                ("minecraft:pale_garden_vegetation".to_owned(), true),
            ]
        );
        assert_eq!(
            features.grass,
            ["minecraft:patch_grass_forest", "patch_tall_grass"]
        );
        // Only the path counts. `pale_garden_vegetation` is Reimagined's keyword (6dd607eb has none).
        assert_eq!(
            features.other,
            [
                "minecraft:flower_default",
                "minecraft:mushroom_island_vegetation",
                "oak:flower",
            ]
        );
        let plain = VegetationFeatures::create(["minecraft:trees_plains"], false);
        assert_eq!(plain.trees, [("minecraft:trees_plains".to_owned(), false)]);
    }

    /// `VegetationFitnessOracle.java` (`.local/terraforged/oracles/vegetation/fitness/`, `run.sh`): each bundled
    /// config's 6dd607eb `getFitness` through 6dd607eb's real `ViabilityContext` (its seed never set: 0) over
    /// [`StubTerrain`]: per maxY and config, the samples strictly inside 0..1 and an FNV-1a fold of all 80.
    const JAVA_FITNESS: &str = "\
F 255 trees_copse 1 367dd6fbafa15045\n\
F 255 trees_hardy 12 5cfeb00a8a6fe63c\n\
F 255 trees_hardy_slopes 57 91ab603a87ab1b26\n\
F 255 trees_patchy 17 b54f189e8c713d11\n\
F 255 trees_rainforest 40 f42c153765cc7b49\n\
F 255 trees_sparse 3 27d43a844b772401\n\
F 255 trees_sparse_rainforest 16 a96eecd158128484\n\
F 255 trees_temperate 27 623bd3fe13617e56\n\
F 480 trees_copse 2 b4588bfe70dccfdd\n\
F 480 trees_hardy 14 82250c99c56be8b5\n\
F 480 trees_hardy_slopes 63 2b45a3ee6ce9f6b8\n\
F 480 trees_patchy 13 c093a18977632ede\n\
F 480 trees_rainforest 21 3583381a3b9891ba\n\
F 480 trees_sparse 3 217c96459345ec69\n\
F 480 trees_sparse_rainforest 14 dc3f44d47bb187d7\n\
F 480 trees_temperate 17 b7afa17f2cd1d2b1\n\
";

    /// The oracle's hash-defined terrain: base 60..91, height base - 32 ..= base + 95, gradient 0..0.02 (scaled and
    /// clamped as `TerrainData.getGradient`), river 0..0.25, no edges, seed 0.
    struct StubTerrain {
        max_y: i32,
    }

    impl super::super::terraforged_biome_viability::ViabilityContext for StubTerrain {
        fn seed(&self) -> i32 {
            0
        }
        fn edge(&self) -> bool {
            false
        }
        fn max_y(&self) -> i32 {
            self.max_y
        }
        fn base_height(&self, x: i32, z: i32) -> i32 {
            60 + (hash_2d(31, x, z) & 31)
        }
        fn height(&self, x: i32, z: i32) -> i32 {
            self.base_height(x, z) + (hash_2d(32, x, z) & 127) - 32
        }
        fn normalized_gradient(&self, x: i32, z: i32, norm: f32) -> f32 {
            let gradient = (hash_2d(33, x, z) & 1023) as f32 / 51_200.0;
            clamp(gradient * norm, 0.0, 1.0)
        }
        fn river(&self, x: i32, z: i32) -> f32 {
            (hash_2d(34, x, z) & 1023) as f32 / 4092.0
        }
        fn climate_shape(&self, _seed: i32, _x: i32, _z: i32) -> f32 {
            0.0
        }
    }

    /// The bundled configs' fitness (the rules with the noise sampled seed-first at the context's seed) matches
    /// 6dd607eb's `getFitness`, bit for bit.
    #[test]
    fn bundled_fitness_matches_6dd607eb_java() {
        let configs = current_vegetation().unwrap();
        let mut lines = JAVA_FITNESS.lines();
        for max_y in [255, 480] {
            let context = StubTerrain { max_y };
            for ((file, _), config) in CONFIGS.iter().zip(&configs) {
                let mut hash = 0xcbf2_9ce4_8422_2325_u64;
                let mut inside = 0;
                for i in 0..80_i32 {
                    let (x, z) = if i < 76 {
                        (
                            ((i * 7919) % 4001 - 2000) * 37 + i,
                            ((i * 104_729) % 3989 - 1994) * 41 - i,
                        )
                    } else {
                        [
                            (i32::MAX, i32::MIN),
                            (i32::MIN, i32::MAX),
                            (30_000_000, -29_999_999),
                            (-30_000_000, 29_999_999),
                        ][(i - 76) as usize]
                    };
                    let value = config.viability.try_fitness(x, z, &context).unwrap();
                    hash = (hash ^ u64::from(bits(value))).wrapping_mul(0x100_0000_01b3);
                    if value > 0.0 && value < 1.0 {
                        inside += 1;
                    }
                }
                let line = format!("F {max_y} {file} {inside} {hash:x}");
                assert_eq!(Some(line.as_str()), lines.next());
            }
        }
        assert_eq!(lines.next(), None);
    }
}
