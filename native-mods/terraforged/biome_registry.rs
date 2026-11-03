/*
 * MIT License
 *
 * Copyright (c) 2021 TerraForged
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

use super::super::{biome::BiomeId, biome_temperature::BiomeTemperatureSettings};
use super::{
    GeneratorData, terraforged_biome_map::BiomePools, terraforged_engine::BiomeType,
    terraforged_terrain_blend_util::WeightMap,
};
use std::{collections::BTreeSet, sync::Arc};

const IS_OVERWORLD: &str = "minecraft:is_overworld";
const IS_BADLANDS: &str = "minecraft:is_badlands";
const HAS_VILLAGE_DESERT: &str = "minecraft:has_structure/village_desert";
const IS_SAVANNA: &str = "minecraft:is_savanna";
const IS_JUNGLE: &str = "minecraft:is_jungle";
const IS_TAIGA: &str = "minecraft:is_taiga";
const IS_FOREST: &str = "minecraft:is_forest";
#[derive(Clone)]
pub struct RegistryBiome<T> {
    pub value: Arc<T>,
    pub name: Arc<str>,
    pub temperature: f32,
    pub downfall: f32,
    pub has_precipitation: bool,
    tags: BTreeSet<Arc<str>>,
}

impl<T> RegistryBiome<T> {
    pub fn new(
        value: T,
        name: impl Into<Arc<str>>,
        temperature: f32,
        downfall: f32,
        has_precipitation: bool,
        tags: impl IntoIterator<Item = Arc<str>>,
    ) -> Self {
        Self {
            value: Arc::new(value),
            name: name.into(),
            temperature,
            downfall,
            has_precipitation,
            tags: tags.into_iter().collect(),
        }
    }

    fn is(&self, tag: &str) -> bool {
        self.tags.contains(tag)
    }

    fn snowy(&self) -> bool {
        self.has_precipitation && self.temperature < 0.15
    }

    pub fn climate_type(&self) -> BiomeType {
        if self.is(IS_BADLANDS) || self.is(HAS_VILLAGE_DESERT) {
            BiomeType::Desert
        } else if self.is(IS_SAVANNA) {
            BiomeType::Savanna
        } else if self.is(IS_JUNGLE) {
            BiomeType::TropicalRainforest
        } else if self.is(IS_TAIGA) {
            if self.snowy() {
                BiomeType::Tundra
            } else {
                BiomeType::Taiga
            }
        } else if self.is(IS_FOREST) {
            if self.snowy() {
                BiomeType::Tundra
            } else if self.downfall >= 0.8 {
                BiomeType::TemperateRainforest
            } else {
                BiomeType::TemperateForest
            }
        } else {
            BiomeType::Grassland
        }
    }

    /// `ResourceLocation.compareTo`: the path first, then the namespace.
    fn location_key(&self) -> (&str, &str) {
        match self.name.split_once(':') {
            Some((namespace, path)) => (path, namespace),
            None => (self.name.as_ref(), "minecraft"),
        }
    }
}

#[derive(Clone)]
pub struct ClimateWeight {
    pub climate: BiomeType,
    pub biome: Arc<str>,
    pub weight: f32,
}

// `BiomeMapManager`: the overworld biome list and one weighted pool per climate.
pub struct RegistryBiomeMap<T> {
    pub overworld: Vec<Arc<T>>,
    pub pools: BiomePools<T>,
}

impl<T> RegistryBiomeMap<T> {
    /// `BiomeMapManager.getWeightsMap`: configured climate weights first, in their order,
    /// then every unconfigured overworld biome at weight 1 in its classified climate. Empty
    /// pools stay empty (the sampler falls back to plains). An unknown configured biome is
    /// an error, as `orElseThrow` is.
    pub fn build(
        entries: &[RegistryBiome<T>],
        configured: &[ClimateWeight],
    ) -> Result<Self, String> {
        Self::build_with(entries, configured, false)
    }

    /// `build`; with `corrected` (the `corrected_biomes` setting) the unconfigured cave,
    /// ocean, river and beach biomes stay out of the pools. They stay in `overworld`,
    /// the biomes structures may find.
    pub fn build_with(
        entries: &[RegistryBiome<T>],
        configured: &[ClimateWeight],
        corrected: bool,
    ) -> Result<Self, String> {
        let mut overworld: Vec<_> = entries
            .iter()
            .filter(|entry| entry.is(IS_OVERWORLD))
            .collect();
        let mut registered = BTreeSet::new();
        let mut weighted: [Vec<(Arc<T>, f32)>; 11] = std::array::from_fn(|_| Vec::new());

        for assignment in configured {
            let entry = entries
                .iter()
                .find(|entry| entry.name == assignment.biome)
                .ok_or_else(|| format!("unknown climate biome {:?}", assignment.biome))?;
            let pool = &mut weighted[assignment.climate as usize];
            if let Some(existing) = pool
                .iter_mut()
                .find(|(value, _)| Arc::ptr_eq(value, &entry.value))
            {
                existing.1 = assignment.weight;
            } else {
                pool.push((Arc::clone(&entry.value), assignment.weight));
            }
            registered.insert(Arc::clone(&entry.name));
            if !overworld
                .iter()
                .any(|candidate| candidate.name == entry.name)
            {
                overworld.push(entry);
            }
        }

        overworld.sort_by(|left, right| left.location_key().cmp(&right.location_key()));
        for entry in &overworld {
            if registered.contains(&entry.name) || (corrected && !belongs_on_land(entry)) {
                continue;
            }
            weighted[entry.climate_type() as usize].push((Arc::clone(&entry.value), 1.0));
        }

        let mut pools = BiomePools::default();
        for climate in BiomeType::VALUES {
            let (values, weights): (Vec<_>, Vec<_>) = weighted[climate as usize]
                .iter()
                .map(|(value, weight)| (Arc::clone(value), *weight))
                .unzip();
            pools.insert(climate, Arc::new(WeightMap::new(values, &weights)));
        }

        Ok(Self {
            overworld: overworld
                .into_iter()
                .map(|entry| Arc::clone(&entry.value))
                .collect(),
            pools,
        })
    }
}

impl RegistryBiomeMap<BiomeId> {
    pub fn from_generator_data(
        data: &GeneratorData,
        configured: &[ClimateWeight],
    ) -> Result<Self, String> {
        let entries: Vec<_> = data
            .biome_names
            .iter()
            .enumerate()
            .map(|(index, name)| {
                let settings: BiomeTemperatureSettings = data.temperatures[index];
                let tags = data
                    .biome_tags
                    .iter()
                    .filter(|(_, members)| members.iter().any(|member| member == name.as_ref()))
                    .map(|(tag, _)| Arc::<str>::from(tag.as_str()));
                RegistryBiome::new(
                    BiomeId(index as u32),
                    Arc::clone(name),
                    settings.base_temperature,
                    settings.downfall,
                    settings.has_precipitation,
                    tags,
                )
            })
            .collect();
        Self::build_with(
            &entries,
            configured,
            super::terraforged_settings::current().corrected_biomes,
        )
    }
}

/// Underground biomes, which grow in caves (`terraforged:cave` included).
pub const CAVE_BIOMES: [&str; 4] = [
    "minecraft:lush_caves",
    "minecraft:dripstone_caves",
    "minecraft:deep_dark",
    "terraforged:cave",
];

/// Whether a climate pool may place the biome on land: not a cave biome, and not an
/// ocean, river or beach (TerraForged's own overrides place those where they belong).
fn belongs_on_land<T>(entry: &RegistryBiome<T>) -> bool {
    !CAVE_BIOMES.contains(&entry.name.as_ref())
        && ![
            "minecraft:is_ocean",
            "minecraft:is_deep_ocean",
            "minecraft:is_river",
            "minecraft:is_beach",
        ]
        .iter()
        .any(|tag| entry.is(tag))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn biome(
        name: &'static str,
        temperature: f32,
        downfall: f32,
        tags: &[&str],
    ) -> RegistryBiome<&'static str> {
        RegistryBiome::new(
            name,
            name,
            temperature,
            downfall,
            true,
            tags.iter().map(|tag| Arc::<str>::from(*tag)),
        )
    }

    #[test]
    fn classifies_registry_biomes_in_java_order() {
        let fixtures = [
            (
                biome("badlands", 0.0, 1.0, &[IS_BADLANDS]),
                BiomeType::Desert,
            ),
            (
                biome("savanna", 0.0, 1.0, &[IS_SAVANNA]),
                BiomeType::Savanna,
            ),
            (
                biome("jungle", 0.0, 1.0, &[IS_JUNGLE]),
                BiomeType::TropicalRainforest,
            ),
            (
                biome("snowy_taiga", 0.0, 0.4, &[IS_TAIGA]),
                BiomeType::Tundra,
            ),
            (biome("taiga", 0.25, 0.4, &[IS_TAIGA]), BiomeType::Taiga),
            (
                biome("rainforest", 0.7, 0.8, &[IS_FOREST]),
                BiomeType::TemperateRainforest,
            ),
            (
                biome("forest", 0.7, 0.79, &[IS_FOREST]),
                BiomeType::TemperateForest,
            ),
        ];
        for (biome, expected) in fixtures {
            assert_eq!(biome.climate_type(), expected);
        }
    }

    #[test]
    fn untagged_biomes_are_grassland() {
        assert_eq!(
            biome("minecraft:mushroom_fields", 0.9, 1.0, &[]).climate_type(),
            BiomeType::Grassland
        );
        assert_eq!(
            biome("minecraft:snowy_plains", 0.0, 0.5, &[]).climate_type(),
            BiomeType::Grassland
        );
    }

    #[test]
    fn builds_pools_like_biome_map_manager() {
        let entries = [
            biome("minecraft:plains", 0.8, 0.4, &[IS_OVERWORLD]),
            biome("minecraft:swamp", 0.8, 0.9, &[IS_OVERWORLD]),
            biome(
                "minecraft:dark_forest",
                0.7,
                0.8,
                &[IS_OVERWORLD, IS_FOREST],
            ),
            biome("minecraft:ocean", 0.5, 0.5, &[IS_OVERWORLD]),
            biome("minecraft:lush_caves", 0.5, 0.5, &[IS_OVERWORLD]),
            biome("a:zebra", 0.5, 0.5, &[IS_OVERWORLD]),
            biome("z:apple", 0.5, 0.5, &[IS_OVERWORLD]),
            biome("minecraft:the_void", 0.5, 0.5, &[]),
        ];
        let configured = [ClimateWeight {
            climate: BiomeType::Desert,
            biome: Arc::from("minecraft:ocean"),
            weight: 3.0,
        }];
        let map = RegistryBiomeMap::build(&entries, &configured).unwrap();
        let names = |climate| {
            map.pools
                .get(climate)
                .unwrap()
                .values()
                .iter()
                .map(|value| **value)
                .collect::<Vec<_>>()
        };
        // Sorted by path, then namespace; oceans and caves are not filtered out.
        assert_eq!(
            names(BiomeType::Grassland),
            [
                "z:apple",
                "minecraft:lush_caves",
                "minecraft:plains",
                "minecraft:swamp",
                "a:zebra"
            ]
        );
        assert_eq!(names(BiomeType::Desert), ["minecraft:ocean"]);
        assert_eq!(
            names(BiomeType::TemperateRainforest),
            ["minecraft:dark_forest"]
        );
        assert!(names(BiomeType::Alpine).is_empty());
        assert_eq!(map.overworld.len(), 7);

        let missing = [ClimateWeight {
            climate: BiomeType::Desert,
            biome: Arc::from("minecraft:missing"),
            weight: 1.0,
        }];
        assert!(RegistryBiomeMap::build(&entries, &missing).is_err());
    }

    #[test]
    fn corrected_pools_keep_caves_and_water_off_land() {
        let entries = [
            biome("minecraft:plains", 0.8, 0.4, &[IS_OVERWORLD]),
            biome("minecraft:deep_dark", 0.8, 0.4, &[IS_OVERWORLD]),
            biome(
                "minecraft:deep_ocean",
                0.5,
                0.5,
                &[
                    IS_OVERWORLD,
                    "minecraft:is_ocean",
                    "minecraft:is_deep_ocean",
                ],
            ),
            biome(
                "minecraft:river",
                0.5,
                0.5,
                &[IS_OVERWORLD, "minecraft:is_river"],
            ),
            biome(
                "minecraft:beach",
                0.8,
                0.4,
                &[IS_OVERWORLD, "minecraft:is_beach"],
            ),
        ];
        let pooled = |corrected| {
            let map = RegistryBiomeMap::build_with(&entries, &[], corrected).unwrap();
            let mut names: Vec<&str> = BiomeType::VALUES
                .iter()
                .flat_map(|climate| map.pools.get(*climate).unwrap().values().to_vec())
                .map(|value| *value)
                .collect();
            names.sort_unstable();
            (names, map.overworld.len())
        };
        assert_eq!(pooled(false).0.len(), 5);
        assert_eq!(pooled(true), (vec!["minecraft:plains"], 5));
    }
}
