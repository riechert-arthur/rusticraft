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

// Adapted from TerraForged Reimagined (https://github.com/Romoslayer/TerraForged-Reimagined, a24d450):
// `src/main/java/com/terraforged/mod/worldgen/biome/util/BiomeMapManager.java` (`getOverworldBiomes`,
// `getWeightsMap`, `getBiomeWeights`, `buildBiomeMap`, `buildDryBiomeMap`, `buildHighlandBiomeMap`, `filtered`) and
// `BiomeUtil.java` (`getOverworldBiomes`, `BIOME_SORTER`, `isNonSurface`, `isWetland`, `isHighland`,
// `isClimateBiome`, `isWaterPlaced`, `getType`, `getByClimate`, `getByRain`, `getByTemp`, `getPrecipitation`), with
// the default `terraforged:{non_surface,wetland,highland}` tag files. 6dd607eb and the 1.18.2-0.3.1 jar have no dry
// or highland pools, no climate lookup for untagged biomes and keep water and cave biomes in the pools. MIT License,
// Copyright (c) 2020 TerraForged, Copyright (c) 2026 Romoslayer; see `LICENSE-TERRAFORGED-REIMAGINED`.

use super::super::{biome::BiomeId, biome_temperature::BiomeTemperatureSettings};
use super::{
    GeneratorData, terraforged::java_min, terraforged_biome_map::BiomePools,
    terraforged_engine::BiomeType, terraforged_terrain_blend_util::WeightMap,
};
use std::{cmp::Ordering, collections::BTreeSet, sync::Arc};

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
/// `ClimateType.IGNORE`: a key the climate codec skips.
const CLIMATE_IGNORE: &str = "forge:registry_name";

/// `BiomeUtil.MIN_TEMPERATURE` / `MAX_TEMPERATURE`: the base-temperature span `getByClimate` normalizes over.
const MIN_TEMPERATURE: f32 = -0.7;
const MAX_TEMPERATURE: f32 = 1.2;

// Reimagined's bundled tag files (`data/terraforged/tags/worldgen/biome/*.json`, `"replace": false`). Horizon's
// data has no `terraforged:` tags, so these stand in for them; a data pack's own tag adds to them.
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

/// One entry of the biome registry as plain data: a `Holder<Biome>` with its key (`name`, `namespace:path`), its
/// climate record (base temperature, downfall, precipitation) and the biome tags it is in. `value` is the biome's
/// one shared handle: every list and pool built from it holds a clone of this `Arc` (Java compares holders with
/// `==`).
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

    /// `getPrecipitation(biome) == SNOW`: precipitation and a base temperature below 0.15.
    fn snowy(&self) -> bool {
        self.has_precipitation && self.temperature < 0.15
    }

    /// `isNonSurface`: the `terraforged:non_surface` tag (with its bundled defaults), or a `cave/…` / `…_caves` path.
    fn is_non_surface(&self) -> bool {
        let path = identifier(&self.name).1;
        self.is(NON_SURFACE)
            || DEFAULT_NON_SURFACE.contains(&self.name.as_ref())
            || path.starts_with("cave/")
            || path.ends_with("_caves")
    }

    /// `isWaterPlaced`: the ocean, deep ocean, river and beach tags, or `minecraft:stony_shore` (`POSITIONAL`).
    fn is_water_placed(&self) -> bool {
        self.is(IS_OCEAN)
            || self.is(IS_DEEP_OCEAN)
            || self.is(IS_RIVER)
            || self.is(IS_BEACH)
            || identifier(&self.name) == ("minecraft", "stony_shore")
    }

    /// `isClimateBiome`: neither a cave biome nor one the override places by position.
    fn is_climate_biome(&self) -> bool {
        !self.is_non_surface() && !self.is_water_placed()
    }

    fn is_wetland(&self) -> bool {
        self.is(WETLAND) || DEFAULT_WETLANDS.contains(&self.name.as_ref())
    }

    fn is_highland(&self) -> bool {
        self.is(HIGHLAND) || DEFAULT_HIGHLANDS.contains(&self.name.as_ref())
    }

    /// `BiomeUtil.getType`: the first of the badlands/desert-village, savanna, jungle, taiga (snowy → tundra) and
    /// forest (snowy → tundra, downfall ≥ 0.8 → temperate rainforest) tags, else `getByClimate`: the base temperature
    /// normalized over -0.7..1.2 and the downfall over 0..1, looked up in Reimagined's clamped `BiomeType.get`.
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
            } else if f64::from(self.downfall) >= 0.8 {
                BiomeType::TemperateRainforest
            } else {
                BiomeType::TemperateForest
            }
        } else {
            let temperature = normalize(self.temperature, MIN_TEMPERATURE, MAX_TEMPERATURE);
            let moisture = normalize(self.downfall, 0.0, 1.0);
            BiomeType::at_climate_clamped(temperature, moisture)
        }
    }
}

/// `BiomeUtil.normalize` (f32; NaN passes through).
fn normalize(value: f32, minimum: f32, maximum: f32) -> f32 {
    let t = (value - minimum) / (maximum - minimum);
    if t < 0.0 { 0.0 } else { java_min(t, 1.0) }
}

/// `Identifier.parse`'s split: `(namespace, path)`, the namespace `minecraft` when there is no `:`.
fn identifier(name: &str) -> (&str, &str) {
    name.split_once(':').unwrap_or(("minecraft", name))
}

/// `BiomeUtil.BIOME_SORTER` (`Identifier.compareTo`): the path first, then the namespace, each by
/// `String.compareTo` (UTF-16 code units).
fn identifier_order(left: &str, right: &str) -> Ordering {
    let (left_namespace, left_path) = identifier(left);
    let (right_namespace, right_path) = identifier(right);
    left_path
        .encode_utf16()
        .cmp(right_path.encode_utf16())
        .then_with(|| {
            left_namespace
                .encode_utf16()
                .cmp(right_namespace.encode_utf16())
        })
}

/// `Identifier.hashCode`: `31 * namespace.hashCode() + path.hashCode()`.
fn identifier_hash(name: &str) -> i32 {
    let java = |text: &str| {
        text.encode_utf16().fold(0_i32, |hash, unit| {
            hash.wrapping_mul(31).wrapping_add(i32::from(unit))
        })
    };
    let (namespace, path) = identifier(name);
    31_i32
        .wrapping_mul(java(namespace))
        .wrapping_add(java(path))
}

/// The iteration order of a fastutil 8.5.18 `Object2FloatOpenHashMap` (default size 16, load factor 0.75) after
/// putting keys with these hash codes in order (all distinct): linear probing from `HashCommon.mix(hash) & mask`,
/// growth to `arraySize(size + 1, 0.75)` once `size` passes `maxFill`, rehash from the top slot down, and iteration
/// from the top slot down. Returns indices into `hashes`.
fn open_hash_order(hashes: &[i32]) -> Vec<usize> {
    fn slot(hash: i32, mask: usize) -> usize {
        let mixed = hash.wrapping_mul(-1_640_531_527);
        ((mixed ^ ((mixed as u32) >> 16) as i32) as u32 as usize) & mask
    }
    fn place(table: &mut [Option<usize>], hashes: &[i32], index: usize) {
        let mask = table.len() - 1;
        let mut position = slot(hashes[index], mask);
        while table[position].is_some() {
            position = (position + 1) & mask;
        }
        table[position] = Some(index);
    }
    let array_size = |expected: usize| ((expected as f32 / 0.75) as f64).ceil().max(2.0) as usize;
    let max_fill = |n: usize| (((n as f32 * 0.75) as f64).ceil() as usize).min(n - 1);
    let mut table = vec![None; array_size(16).next_power_of_two()];
    for index in 0..hashes.len() {
        place(&mut table, hashes, index);
        if index >= max_fill(table.len()) {
            let mut grown = vec![None; array_size(index + 2).next_power_of_two()];
            for &old in table.iter().rev().flatten() {
                place(&mut grown, hashes, old);
            }
            table = grown;
        }
    }
    table.iter().rev().flatten().copied().collect()
}

/// One weight of a climate file (`data/terraforged/worldgen/climate/<climate>.json`, a `ClimateType`): `biome` is
/// the key as `Identifier.parse` reads it (no namespace = `minecraft`; `forge:registry_name` is skipped, as the
/// codec does). Pass a file's entries in file order, one per JSON key; where two keys name one biome (`plains` and
/// `minecraft:plains`), the later key's weight wins. The pool takes each file's biomes in `ClimateType`'s hash-map order, not
/// file order (see [`RegistryBiomeMap::build`]).
#[derive(Clone)]
pub struct ClimateWeight {
    pub climate: BiomeType,
    pub biome: Arc<str>,
    pub weight: f32,
}

/// `BiomeMapManager`: the overworld biome list (`Source.possibleBiomes`) and the three pool maps.
pub struct RegistryBiomeMap<T> {
    pub overworld: Vec<Arc<T>>,
    pub wet: BiomePools<T>,
    pub dry: BiomePools<T>,
    pub highland: BiomePools<T>,
}

impl<T> RegistryBiomeMap<T> {
    /// `new BiomeMapManager(access)` over `entries` (the biome registry) and `configured` (the climate files).
    ///
    /// - `overworld`: every `minecraft:is_overworld` biome plus every registry biome a climate file names, sorted by
    ///   `BIOME_SORTER` (path, then namespace).
    /// - `wet` (`getBiomeMap`): per climate type, first its file's biomes that are in the registry (others are
    ///   skipped), with their weights, in the file's hash-map order; then, in `overworld` order at weight 1, every
    ///   biome no file names that `isClimateBiome` (not a cave or water biome), in its `getType` climate. Then empty
    ///   climates borrow the nearest one's pool ([`BiomePools::fill_empty_climates`]).
    /// - `dry`: each pool without its wetland biomes, weights 1; a climate left with none keeps its wet pool.
    /// - `highland`: each pool's highland biomes only, weights 1; climates left with none borrow, as above.
    pub fn build(entries: &[RegistryBiome<T>], configured: &[ClimateWeight]) -> Self {
        let mut overworld: Vec<_> = entries
            .iter()
            .filter(|entry| entry.is(IS_OVERWORLD))
            .collect();
        let mut registered = BTreeSet::new();
        let mut weighted: [Vec<(Arc<T>, f32)>; 11] = std::array::from_fn(|_| Vec::new());

        for climate in BiomeType::VALUES {
            // The file as `ClimateType`'s `Object2FloatOpenHashMap<Identifier>`: one entry per key, last weight.
            let mut file: Vec<(&str, f32)> = Vec::new();
            for assignment in configured
                .iter()
                .filter(|a| a.climate == climate && a.biome.as_ref() != CLIMATE_IGNORE)
            {
                let key = identifier(&assignment.biome);
                match file.iter_mut().find(|(name, _)| identifier(name) == key) {
                    Some(existing) => existing.1 = assignment.weight,
                    None => file.push((&assignment.biome, assignment.weight)),
                }
            }
            let hashes: Vec<_> = file.iter().map(|(name, _)| identifier_hash(name)).collect();
            for index in open_hash_order(&hashes) {
                let (name, weight) = file[index];
                let key = identifier(name);
                let Some(entry) = entries.iter().find(|entry| identifier(&entry.name) == key)
                else {
                    continue;
                };
                weighted[climate as usize].push((Arc::clone(&entry.value), weight));
                registered.insert(Arc::clone(&entry.name));
                if !overworld
                    .iter()
                    .any(|known| Arc::ptr_eq(&known.value, &entry.value))
                {
                    overworld.push(entry);
                }
            }
        }

        overworld.sort_by(|left, right| identifier_order(&left.name, &right.name));
        for entry in &overworld {
            if registered.contains(&entry.name) || !entry.is_climate_biome() {
                continue;
            }
            weighted[entry.climate_type() as usize].push((Arc::clone(&entry.value), 1.0));
        }

        let mut wet = BiomePools::default();
        for climate in BiomeType::VALUES {
            let (values, weights): (Vec<_>, Vec<_>) =
                std::mem::take(&mut weighted[climate as usize])
                    .into_iter()
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

/// `BiomeMapManager.filtered`: each climate's pool rebuilt from the biomes that pass `keep`, weights 1. A climate left
/// with none keeps the very same pool when `keep_original_if_empty`, else has no entry.
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
            let weights = vec![1.0; kept.len()];
            result.insert(climate, Arc::new(WeightMap::new(kept, &weights)));
        }
    }
    result
}

impl RegistryBiomeMap<BiomeId> {
    /// [`RegistryBiomeMap::build`] over Horizon's loaded biome registry ([`registry_entries`]).
    pub fn from_generator_data(data: &GeneratorData, configured: &[ClimateWeight]) -> Self {
        Self::build(&registry_entries(data), configured)
    }
}

/// Horizon's loaded biome registry as [`RegistryBiome`]s: one per biome id, with its climate settings and the
/// (resolved) biome tags that list it.
fn registry_entries(data: &GeneratorData) -> Vec<RegistryBiome<BiomeId>> {
    data.biome_names
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
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pools_share_one_arc_per_biome() {
        let forest = [IS_OVERWORLD, IS_FOREST];
        let entries = [
            spec("swamp", "minecraft:swamp", 0.8, 0.9, true, &forest),
            spec(
                "hills",
                "minecraft:windswept_hills",
                0.7,
                0.9,
                true,
                &forest,
            ),
        ];
        let map = RegistryBiomeMap::build(&entries, &[]);
        let wet = map.wet.get(BiomeType::TemperateRainforest).unwrap();
        let dry = map.dry.get(BiomeType::TemperateRainforest).unwrap();
        let highland = map.highland.get(BiomeType::TemperateRainforest).unwrap();
        assert!(Arc::ptr_eq(&wet.values()[1], &dry.values()[0]));
        assert!(Arc::ptr_eq(&wet.values()[1], &highland.values()[0]));
        assert!(Arc::ptr_eq(&map.overworld[1], &entries[1].value));
    }

    // ---- BiomeRegistryOracle (`.local/terraforged/oracles/biome_registry/`): Reimagined's real BiomeMapManager ----

    /// The oracle's `H` lines: (section, lines, FNV-1a 64 of the section's lines).
    const JAVA: [(&str, usize, u64); 6] = [
        ("vanilla", 35, 0xcf8a_bfce_a2f1_bc9b),
        ("modded", 35, 0xc7cb_ca0d_9d1e_1b39),
        ("files", 35, 0xc3b7_7244_4eeb_7c1d),
        ("untagged", 35, 0x2151_f6a2_4812_3642),
        ("none", 35, 0x44c1_d955_2609_16a0),
        ("random", 140_000, 0xb70e_6528_e966_a90e),
    ];
    const RANDOM_CASES: i64 = 4000;

    struct Section(usize, u64, String);
    impl Section {
        fn new() -> Self {
            Self(0, 0xcbf2_9ce4_8422_2325, String::new())
        }
        fn emit(&mut self, line: String) {
            for byte in line.bytes().chain([b'\n']) {
                self.1 = (self.1 ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
            }
            self.0 += 1;
            self.2 += &line;
            self.2.push('\n');
        }
        fn check(&self, section: &str) {
            if let Some(path) = std::env::var_os("TF_BIOME_REGISTRY_DUMP") {
                let path = std::path::Path::new(&path).join(section);
                std::fs::write(path, &self.2).unwrap();
            }
            let java = JAVA.iter().find(|(name, ..)| *name == section).unwrap();
            assert_eq!((self.0, self.1), (java.1, java.2), "{section}");
        }
        fn case<T>(&mut self, case: &str, map: &RegistryBiomeMap<T>, name: impl Fn(&T) -> String) {
            self.emit(format!("C {case}"));
            let mut line = String::from("O");
            for value in &map.overworld {
                line = line + " " + &name(value);
            }
            self.emit(line);
            for (kind, pools) in [("w", &map.wet), ("d", &map.dry), ("h", &map.highland)] {
                for climate in BiomeType::VALUES {
                    let Some(pool) = pools.get(climate) else {
                        self.emit(format!("P {kind} {} -", climate as usize));
                        continue;
                    };
                    let share = BiomeType::VALUES
                        .iter()
                        .position(|c| pools.get(*c).is_some_and(|p| Arc::ptr_eq(p, pool)))
                        .unwrap();
                    let mut line = format!("P {kind} {} s{share}", climate as usize);
                    if kind == "d" && map.wet.get(climate).is_some_and(|w| Arc::ptr_eq(w, pool)) {
                        line.push('w');
                    }
                    for value in pool.values() {
                        line += &format!(" {}={:x}", name(value), pool.band(value) as u64);
                    }
                    self.emit(line);
                }
            }
        }
    }

    fn spec<T>(value: T, name: &str, t: f32, d: f32, p: bool, tags: &[&str]) -> RegistryBiome<T> {
        let tags = tags.iter().map(|tag| Arc::<str>::from(*tag));
        RegistryBiome::new(value, name, t, d, p, tags)
    }

    /// The oracle's `MODS`: name, temperature, downfall, precipitation, tags (o overworld, f forest, t taiga, c ocean,
    /// h highland, w wetland).
    const MODS: &str = "terralith:alpine_grove -0.2 0.5 1 o, terralith:sandstone_valley 1.5 0.0 0 o, \
        terralith:cave/frostfire_caves 0.0 0.5 1 o, terralith:moonlight_grove 0.6 0.85 1 of, \
        terralith:skylands 0.5 0.5 1 oh, terralith:yellowstone 0.9 0.4 1 o, biomesoplenty:bayou 0.8 0.95 1 ow, \
        biomesoplenty:tundra -0.3 0.3 1 o, biomesoplenty:cold_desert 0.0 0.0 0 o, biomesoplenty:lagoon 0.5 0.5 1 oc, \
        biomesoplenty:crag 0.3 0.2 1 oth, biomesoplenty:not_overworld 0.8 0.4 1 -, a:plains 0.8 0.4 1 o";

    fn mods() -> impl Iterator<Item = (&'static str, f32, f32, bool, Vec<&'static str>)> {
        let tag = |code| match code {
            'o' => IS_OVERWORLD,
            'f' => IS_FOREST,
            't' => IS_TAIGA,
            'c' => IS_OCEAN,
            'h' => HIGHLAND,
            'w' => WETLAND,
            _ => "minecraft:other",
        };
        MODS.split(", ").map(move |row| {
            let f: Vec<_> = row.split_whitespace().collect();
            let tags = f[4].chars().filter(|c| *c != '-').map(tag).collect();
            (
                f[0],
                f[1].parse().unwrap(),
                f[2].parse().unwrap(),
                f[3] == "1",
                tags,
            )
        })
    }

    fn weights(climate: BiomeType, entries: &[(&str, f32)]) -> Vec<ClimateWeight> {
        let weight = |&(biome, weight): &(&str, f32)| ClimateWeight {
            climate,
            biome: Arc::from(biome),
            weight,
        };
        entries.iter().map(weight).collect()
    }

    /// The oracle's `files()`: desert (unknown, bare, ignored and non-overworld keys), a 30-entry grassland (the hash
    /// map grows), an empty taiga file and a one-entry tundra file.
    fn files() -> Vec<ClimateWeight> {
        let grassland = "plains sunflower_plains meadow forest birch_forest dark_forest flower_forest cherry_grove \
            savanna taiga swamp jungle badlands desert snowy_plains ice_spikes grove stony_peaks windswept_hills \
            old_growth_birch_forest terralith:yellowstone terralith:alpine_grove biomesoplenty:tundra a:plains \
            mushroom_fields sparse_jungle bamboo_jungle mangrove_swamp pale_garden wooded_badlands";
        let grassland: Vec<String> = grassland
            .split_whitespace()
            .map(|b| {
                if b.contains(':') {
                    b.to_owned()
                } else {
                    format!("minecraft:{b}")
                }
            })
            .collect();
        let grassland: Vec<_> = (0..grassland.len())
            .map(|i| (grassland[i].as_str(), (i % 7) as f32))
            .collect();
        let desert = [
            ("terralith:sandstone_valley", 3.0),
            ("minecraft:desert", 5.0),
            ("plains", 1.0),
            ("missingmod:foo", 2.0),
            (CLIMATE_IGNORE, 7.0),
            ("biomesoplenty:not_overworld", 0.5),
            ("minecraft:plains", 4.0),
        ];
        let mut all = weights(BiomeType::Desert, &desert);
        all.extend(weights(BiomeType::Grassland, &grassland));
        all.extend(weights(
            BiomeType::Tundra,
            &[("terralith:alpine_grove", 2.0)],
        ));
        all
    }

    #[test]
    fn vanilla_and_modded_registries_match_reimagined_java() {
        let root = std::env::var_os("HORIZON_TEST_RESOURCES")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../.local/horizon-client/26.3")
            });
        if !root.join("worldgen/world_presets.json").exists() {
            eprintln!("skipping: no worldgen bundle at {}", root.display());
            return;
        }
        let registry = Box::leak(Box::new(crate::level::data::load_registry(&root).unwrap()));
        let data = GeneratorData::load(&root.join("worldgen"), registry).unwrap();
        let mut names: Vec<String> = data.biome_names.iter().map(|n| n.to_string()).collect();
        let vanilla = registry_entries(&data);
        let mut modded = vanilla.clone();
        for (name, t, d, p, tags) in mods() {
            modded.push(spec(BiomeId(names.len() as u32), name, t, d, p, &tags));
            names.push(name.to_owned());
        }
        let name = |id: &BiomeId| names[id.0 as usize].clone();
        let strip = |keep: bool| -> Vec<_> {
            let mut entries = vanilla.clone();
            for entry in &mut entries {
                entry
                    .tags
                    .retain(|tag| keep && tag.as_ref() == IS_OVERWORLD);
            }
            entries
        };
        let cases: [(&str, &[RegistryBiome<BiomeId>], Vec<ClimateWeight>); 5] = [
            ("vanilla", &vanilla, Vec::new()),
            ("modded", &modded, Vec::new()),
            ("files", &modded, files()),
            ("untagged", &strip(true), Vec::new()),
            ("none", &strip(false), Vec::new()),
        ];
        for (case, entries, configured) in cases {
            let mut section = Section::new();
            let map = if case == "vanilla" {
                RegistryBiomeMap::from_generator_data(&data, &configured)
            } else {
                RegistryBiomeMap::build(entries, &configured)
            };
            section.case(case, &map, name);
            section.check(case);
        }
    }

    /// The oracle's `Gen` LCG.
    struct Gen(u64);
    impl Gen {
        fn next(&mut self, n: usize) -> usize {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            ((self.0 >> 33) % n as u64) as usize
        }
    }

    #[test]
    fn random_registries_match_reimagined_java() {
        let ns: Vec<_> = "minecraft terralith biomesoplenty a zz terraforged"
            .split(' ')
            .collect();
        let paths: Vec<_> = "plains forest cave/deep lush_caves ocean stony_shore swamp windswept_hills badlands a \
            b/c z plains_caves river beach snowy_taiga jungle savanna mangrove_swamp grove deep_dark cave alpine \
            x_caves_y a-b a.b a_b ab cave/ caves"
            .split_whitespace()
            .collect();
        let tags = [
            IS_BADLANDS,
            HAS_VILLAGE_DESERT,
            IS_SAVANNA,
            IS_JUNGLE,
            IS_TAIGA,
            IS_FOREST,
            IS_OCEAN,
        ];
        let tags = tags.into_iter().chain([
            IS_DEEP_OCEAN,
            IS_RIVER,
            IS_BEACH,
            NON_SURFACE,
            WETLAND,
            HIGHLAND,
        ]);
        let tags_pool: Vec<_> = tags.chain(["minecraft:other"]).collect();
        // Java float literals (`Float.MIN_VALUE` is 1e-45's nearest float).
        let parse =
            |text: &str| -> Vec<f32> { text.split(' ').map(|v| v.parse().unwrap()).collect() };
        let floats = parse(
            "-2.0 -0.7 -0.5 0.0 -0.0 0.1 0.14999999 0.15 0.15000001 0.2 0.25 0.3 0.4 0.5 0.6 0.7 0.79999995 \
             0.8 0.8000001 0.9 0.95 1.0 1.2 1.5 2.0 NaN inf -inf 1e-45",
        );
        let weights = parse("1 5 0 0.5 3 2.5 NaN 1e-45 100 -1");
        let mut section = Section::new();
        for case in 0..RANDOM_CASES {
            let mut g = Gen((case as u64)
                .wrapping_mul(0x9E37_79B9_7F4A_7C15)
                .wrapping_add(1));
            let mut name =
                |g: &mut Gen| format!("{}:{}", ns[g.next(ns.len())], paths[g.next(paths.len())]);
            let mut specs: Vec<RegistryBiome<String>> = Vec::new();
            for _ in 0..g.next(25) {
                let biome = name(&mut g);
                let (t, d) = (floats[g.next(floats.len())], floats[g.next(floats.len())]);
                let p = g.next(4) != 0;
                let mut tags = Vec::new();
                if g.next(4) != 0 {
                    tags.push(IS_OVERWORLD);
                }
                tags.extend(tags_pool.iter().copied().filter(|_| g.next(7) == 0));
                if specs.iter().all(|s| *s.name != *biome) {
                    specs.push(spec(biome.clone(), &biome, t, d, p, &tags));
                }
            }
            let mut configured = Vec::new();
            for climate in BiomeType::VALUES {
                let file = configured.len();
                let count = match g.next(5) {
                    0 | 1 => 0,
                    4 => 20 + g.next(25),
                    _ => 1 + g.next(6),
                };
                for _ in 0..count {
                    let pick = g.next(8);
                    let biome = if pick == 0 {
                        CLIMATE_IGNORE.to_owned()
                    } else if pick < 4 && !specs.is_empty() {
                        let biome = specs[g.next(specs.len())].name.to_string();
                        match biome.strip_prefix("minecraft:") {
                            Some(path) if g.next(2) == 0 => path.to_owned(),
                            _ => biome,
                        }
                    } else {
                        name(&mut g)
                    };
                    let weight = weights[g.next(weights.len())];
                    // One entry per JSON key, as the oracle's climate files.
                    if !configured[file..]
                        .iter()
                        .any(|w: &ClimateWeight| *w.biome == *biome)
                    {
                        configured.push(ClimateWeight {
                            climate,
                            biome: Arc::from(biome),
                            weight,
                        });
                    }
                }
            }
            let map = RegistryBiomeMap::build(&specs, &configured);
            section.case(&case.to_string(), &map, String::clone);
        }
        section.check("random");
    }
}
