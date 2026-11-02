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

// `VegetationConfig` and `ViabilityCodec` over the bundled `worldgen/vegetation`
// resources and TerraForged's `trees/*` biome tags (from the 0.3.1 release jar, whose
// data generator wrote them), `BiomeVegetationManager`'s config lookup and
// `VegetationFeatures`' split of a biome's vegetal features.

use super::terraforged::Module;
use super::terraforged_biome_viability::Viability;
use super::terraforged_terrain_asset::decode_module;
use serde_json::Value;
use std::collections::BTreeSet;

pub struct VegetationConfig {
    pub frequency: f32,
    pub jitter: f32,
    pub density: f32,
    /// The biome tag's members.
    pub biomes: BTreeSet<String>,
    pub viability: Viability,
}

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

/// The bundled configs in registry order (`TerraForged.VEGETATIONS.entries` sorts by id).
pub fn current_vegetation() -> Result<Vec<VegetationConfig>, String> {
    CONFIGS
        .iter()
        .map(|(name, json)| {
            decode_config(text(json)).map_err(|error| format!("vegetation {name}: {error}"))
        })
        .collect()
}

/// `BiomeVegetationManager.getConfig`: the first config whose tag holds the biome.
pub fn config_for<'a>(
    configs: &'a [VegetationConfig],
    biome: &str,
) -> Option<&'a VegetationConfig> {
    configs.iter().find(|config| config.biomes.contains(biome))
}

pub fn decode_config(json: &str) -> Result<VegetationConfig, String> {
    let value: Value = serde_json::from_str(json).map_err(|error| error.to_string())?;
    let number = |name: &str| value.get(name).map_or(Ok(1.0), |v| float(v, name));
    let tag = value["biomes"]
        .as_str()
        .and_then(|tag| tag.strip_prefix('#'))
        .ok_or("biomes must be a tag")?;
    Ok(VegetationConfig {
        frequency: number("frequency")?,
        jitter: number("jitter")?,
        density: number("density")?,
        biomes: tag_members(tag)?,
        viability: decode_viability(&value["viability"])?,
    })
}

fn tag_members(tag: &str) -> Result<BTreeSet<String>, String> {
    let (_, json) = TAGS
        .iter()
        .find(|(name, _)| *name == tag)
        .ok_or_else(|| format!("unknown biome tag {tag:?}"))?;
    let value: Value = serde_json::from_str(text(json)).map_err(|error| error.to_string())?;
    value["values"]
        .as_array()
        .ok_or("tag values must be an array")?
        .iter()
        .map(|entry| {
            entry
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| "tag value must be a string".into())
        })
        .collect()
}

/// `ViabilityCodec`: each spec's fields with its defaults.
pub fn decode_viability(value: &Value) -> Result<Viability, String> {
    let number = |name: &str, default: f32| value.get(name).map_or(Ok(default), |v| float(v, name));
    let rules = |name: &str| -> Result<Vec<Viability>, String> {
        value
            .get(name)
            .and_then(Value::as_array)
            .map_or(Ok(Vec::new()), |rules| {
                rules.iter().map(decode_viability).collect()
            })
    };
    Ok(
        match value["type"].as_str().ok_or("viability needs a type")? {
            "BiomeEdgeViability" => Viability::BiomeEdge {
                distance: number("distance", 1.0)?,
            },
            "Height" => Viability::Height {
                min_offset: number("min", 0.0)?,
                mid_offset: number("mid", 0.5)?,
                max_offset: number("max", 1.0)?,
            },
            "Multiply" => Viability::Multiply(rules("rules")?),
            "Noise" => Viability::Noise(noise(&value["noise"])?),
            "Saturation" => Viability::Saturation {
                min: number("min", 0.0)?,
                max: number("max", 1.0)?,
            },
            "Slope" => Viability::Slope {
                normalize: number("normalize", 1.0)?,
                max: number("max", 1.0)?,
            },
            "Sum" => Viability::Sum {
                initial: number("initial", 1.0)?,
                rules: rules("rules")?,
                amounts: value.get("amounts").and_then(Value::as_array).map_or(
                    Ok(Vec::new()),
                    |amounts| {
                        amounts
                            .iter()
                            .map(|amount| float(amount, "amounts"))
                            .collect()
                    },
                )?,
            },
            other => return Err(format!("unknown viability {other:?}")),
        },
    )
}

fn noise(value: &Value) -> Result<Module, String> {
    decode_module(value).map_err(|error| error.to_string())
}

fn float(value: &Value, name: &str) -> Result<f32, String> {
    value
        .as_f64()
        .map(|value| value as f32)
        .ok_or_else(|| format!("{name} must be a number"))
}

fn text(bytes: &'static [u8]) -> &'static str {
    std::str::from_utf8(bytes).expect("TerraForged vegetation JSON is UTF-8")
}

/// `VegetationFeatures.create`: a biome's `VEGETAL_DECORATION` features split by their
/// ids into trees, grass and the rest, as `(feature, unwrap)` where `unwrap` asks for the
/// tree's placement without its count, square and biome modifiers (custom configs only).
pub struct VegetationFeatures {
    pub trees: Vec<(String, bool)>,
    pub grass: Vec<String>,
    pub other: Vec<String>,
}

const TREE_KEYWORDS: [&str; 6] = [
    "tree",
    "spruce",
    "oak",
    "birch",
    "pine",
    "dark_forest_vegetation",
];
const GRASS_KEYWORDS: [&str; 1] = ["grass"];

impl VegetationFeatures {
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
    use super::*;

    #[test]
    fn bundled_configs_decode_in_registry_order() {
        let configs = current_vegetation().unwrap();
        assert_eq!(configs.len(), 8);
        let temperate = config_for(&configs, "minecraft:birch_forest").unwrap();
        assert_eq!(temperate.frequency.to_bits(), 0.2_f32.to_bits());
        assert_eq!(temperate.density.to_bits(), 0.6_f32.to_bits());
        assert!(
            matches!(&temperate.viability, Viability::Sum { rules, amounts, .. } if rules.len() == 5 && amounts.len() == 5)
        );
        // trees_hardy comes before trees_hardy_slopes, so taiga uses hardy.
        let taiga = config_for(&configs, "minecraft:taiga").unwrap();
        assert!(taiga.biomes.contains("minecraft:dark_forest"));
        assert!(config_for(&configs, "minecraft:desert").is_none());
    }

    #[test]
    fn vegetal_features_split_like_terraforged() {
        let features = VegetationFeatures::create(
            [
                "minecraft:trees_birch_and_oak_leaf_litter",
                "minecraft:patch_grass_forest",
                "minecraft:flower_default",
                "minecraft:dark_forest_vegetation",
            ],
            true,
        );
        assert_eq!(features.trees.len(), 2);
        assert!(features.trees.iter().all(|(_, unwrap)| *unwrap));
        assert_eq!(features.grass, ["minecraft:patch_grass_forest"]);
        assert_eq!(features.other, ["minecraft:flower_default"]);
    }
}
