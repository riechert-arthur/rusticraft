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
const IS_OCEAN: &str = "minecraft:is_ocean";
const IS_DEEP_OCEAN: &str = "minecraft:is_deep_ocean";
const IS_RIVER: &str = "minecraft:is_river";
const IS_BEACH: &str = "minecraft:is_beach";
const NON_SURFACE: &str = "terraforged:non_surface";
const WETLAND: &str = "terraforged:wetland";
const HIGHLAND: &str = "terraforged:highland";

const DEFAULT_WETLANDS: &[&str] = &["minecraft:swamp", "minecraft:mangrove_swamp"];
const DEFAULT_HIGHLANDS: &[&str] = &[
    "minecraft:windswept_hills",
    "minecraft:windswept_gravelly_hills",
    "minecraft:windswept_forest",
    "minecraft:windswept_savanna",
    "minecraft:stony_peaks",
    "minecraft:snowy_slopes",
    "minecraft:jagged_peaks",
    "minecraft:frozen_peaks",
    "minecraft:grove",
    "minecraft:badlands",
    "minecraft:eroded_badlands",
    "minecraft:wooded_badlands",
];
const DEFAULT_NON_SURFACE: &[&str] = &[
    "minecraft:deep_dark",
    "minecraft:dripstone_caves",
    "minecraft:lush_caves",
    "minecraft:sulfur_caves",
    "terraforged:cave",
];

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

    fn is_non_surface(&self) -> bool {
        self.is(NON_SURFACE)
            || DEFAULT_NON_SURFACE.contains(&self.name.as_ref())
            || self
                .name
                .split_once(':')
                .is_some_and(|(_, path)| path.starts_with("cave/") || path.ends_with("_caves"))
    }

    fn is_water_placed(&self) -> bool {
        self.is(IS_OCEAN)
            || self.is(IS_DEEP_OCEAN)
            || self.is(IS_RIVER)
            || self.is(IS_BEACH)
            || self.name.as_ref() == "minecraft:stony_shore"
    }

    fn is_climate_biome(&self) -> bool {
        !self.is_non_surface() && !self.is_water_placed()
    }

    fn is_wetland(&self) -> bool {
        self.is(WETLAND) || DEFAULT_WETLANDS.contains(&self.name.as_ref())
    }

    fn is_highland(&self) -> bool {
        self.is(HIGHLAND) || DEFAULT_HIGHLANDS.contains(&self.name.as_ref())
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
            let temperature = normalize(self.temperature, -0.7, 1.2);
            let moisture = normalize(self.downfall, 0.0, 1.0);
            BiomeType::at_climate(temperature, moisture)
        }
    }
}

fn normalize(value: f32, minimum: f32, maximum: f32) -> f32 {
    ((value - minimum) / (maximum - minimum)).clamp(0.0, 1.0)
}

#[derive(Clone)]
pub struct ClimateWeight {
    pub climate: BiomeType,
    pub biome: Arc<str>,
    pub weight: f32,
}

pub struct RegistryBiomeMap<T> {
    pub overworld: Vec<Arc<T>>,
    pub wet: BiomePools<T>,
    pub dry: BiomePools<T>,
    pub highland: BiomePools<T>,
}

impl<T> RegistryBiomeMap<T> {
    pub fn build(entries: &[RegistryBiome<T>], configured: &[ClimateWeight]) -> Self {
        let mut overworld: Vec<_> = entries
            .iter()
            .filter(|entry| entry.is(IS_OVERWORLD))
            .collect();
        let mut registered = BTreeSet::new();
        let mut weighted: [Vec<(Arc<T>, f32)>; 11] = std::array::from_fn(|_| Vec::new());

        for assignment in configured {
            let Some(entry) = entries.iter().find(|entry| entry.name == assignment.biome) else {
                continue;
            };
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

        overworld.sort_by(|left, right| left.name.cmp(&right.name));
        for entry in &overworld {
            if registered.contains(&entry.name) || !entry.is_climate_biome() {
                continue;
            }
            weighted[entry.climate_type() as usize].push((Arc::clone(&entry.value), 1.0));
        }

        let mut wet = BiomePools::default();
        for climate in BiomeType::VALUES {
            let (values, weights): (Vec<_>, Vec<_>) = weighted[climate as usize]
                .iter()
                .map(|(value, weight)| (Arc::clone(value), *weight))
                .unzip();
            wet.insert(climate, Arc::new(WeightMap::new(values, &weights)));
        }
        wet.fill_empty_climates();

        let dry = filtered_pools(&wet, entries, |entry| !entry.is_wetland(), true);
        let mut highland = filtered_pools(&wet, entries, RegistryBiome::is_highland, false);
        highland.fill_empty_climates();

        Self {
            overworld: overworld
                .into_iter()
                .map(|entry| Arc::clone(&entry.value))
                .collect(),
            wet,
            dry,
            highland,
        }
    }
}

fn filtered_pools<T>(
    source: &BiomePools<T>,
    entries: &[RegistryBiome<T>],
    keep: impl Fn(&RegistryBiome<T>) -> bool,
    keep_original_if_empty: bool,
) -> BiomePools<T> {
    let mut result = BiomePools::default();
    for climate in BiomeType::VALUES {
        let Some(original) = source.get(climate) else {
            continue;
        };
        let kept: Vec<_> = original
            .values()
            .iter()
            .filter(|value| {
                entries
                    .iter()
                    .find(|entry| Arc::ptr_eq(&entry.value, value))
                    .is_some_and(&keep)
            })
            .cloned()
            .collect();
        if kept.is_empty() {
            if keep_original_if_empty {
                result.insert(climate, Arc::clone(original));
            }
        } else {
            result.insert(
                climate,
                Arc::new(WeightMap::new(kept.clone(), &vec![1.0; kept.len()])),
            );
        }
    }
    result
}

impl RegistryBiomeMap<BiomeId> {
    pub fn from_generator_data(data: &GeneratorData, configured: &[ClimateWeight]) -> Self {
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
        Self::build(&entries, configured)
    }
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
    fn builds_configured_and_filtered_registry_pools() {
        let entries = [
            biome("minecraft:plains", 0.8, 0.4, &[IS_OVERWORLD]),
            biome("minecraft:swamp", 0.8, 0.9, &[IS_OVERWORLD, IS_FOREST]),
            biome(
                "minecraft:windswept_hills",
                0.2,
                0.3,
                &[IS_OVERWORLD, IS_FOREST],
            ),
            biome(
                "minecraft:dark_forest",
                0.7,
                0.8,
                &[IS_OVERWORLD, IS_FOREST],
            ),
            biome("minecraft:ocean", 0.5, 0.5, &[IS_OVERWORLD, IS_OCEAN]),
            biome("minecraft:lush_caves", 0.5, 0.5, &[IS_OVERWORLD]),
        ];
        let configured = [ClimateWeight {
            climate: BiomeType::Desert,
            biome: Arc::from("minecraft:ocean"),
            weight: 3.0,
        }];
        let map = RegistryBiomeMap::build(&entries, &configured);

        assert_eq!(map.overworld.len(), 6);
        assert_eq!(
            map.wet.get(BiomeType::Desert).unwrap().values()[0].as_ref(),
            &"minecraft:ocean"
        );
        assert!(
            map.wet
                .get(BiomeType::TemperateRainforest)
                .unwrap()
                .values()
                .iter()
                .any(|value| value.as_ref() == &"minecraft:swamp")
        );
        assert!(
            !map.dry
                .get(BiomeType::TemperateRainforest)
                .unwrap()
                .values()
                .iter()
                .any(|value| value.as_ref() == &"minecraft:swamp")
        );
        assert_eq!(
            map.highland
                .get(BiomeType::TemperateForest)
                .unwrap()
                .values()[0]
                .as_ref(),
            &"minecraft:windswept_hills"
        );
    }
}
