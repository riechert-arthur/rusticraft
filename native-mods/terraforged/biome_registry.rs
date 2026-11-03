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

// TerraForged 6dd607eb's `BiomeMapManager` (`getOverworldBiomes`, `getWeightsMap`, `getBiomeWeights`,
// `buildBiomeMap`) and `BiomeUtil` (`getOverworldBiomes`, `BIOME_SORTER`, `getType`, `getByRain`, `getByTemp`), in
// `src/main/java/com/terraforged/mod/worldgen/biome/util/`, with `ClimateType`'s weight map
// (`src/main/java/com/terraforged/mod/worldgen/asset/ClimateType.java`); MIT, see `LICENSE-TERRAFORGED`. 1.18's
// `Biome.getPrecipitation()` is gone in 26.3: its position-free stand-in (`snowy`) is TerraForged Reimagined
// a24d450's `BiomeUtil.getPrecipitation` (https://github.com/Romoslayer/TerraForged-Reimagined, MIT License,
// Copyright (c) 2020 TerraForged, Copyright (c) 2026 Romoslayer; see `LICENSE-TERRAFORGED-REIMAGINED`).

use super::super::{biome::BiomeId, biome_temperature::BiomeTemperatureSettings};
use super::{
    GeneratorData, terraforged_biome_map::BiomePools, terraforged_engine::BiomeType,
    terraforged_terrain_blend_util::WeightMap,
};
use std::{cmp::Ordering, collections::BTreeSet, sync::Arc};

const IS_OVERWORLD: &str = "minecraft:is_overworld";
const IS_BADLANDS: &str = "minecraft:is_badlands";
const HAS_VILLAGE_DESERT: &str = "minecraft:has_structure/village_desert";
const IS_SAVANNA: &str = "minecraft:is_savanna";
const IS_JUNGLE: &str = "minecraft:is_jungle";
const IS_TAIGA: &str = "minecraft:is_taiga";
const IS_FOREST: &str = "minecraft:is_forest";
/// `ClimateType.IGNORE`: a key the climate codec skips.
const CLIMATE_IGNORE: &str = "forge:registry_name";

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

    /// `biome.getPrecipitation() == SNOW`. 1.18 stored the precipitation in the biome; 26.3 keeps only
    /// `hasPrecipitation` and decides snow per position, so this is Reimagined's position-free rule: precipitation
    /// and a base temperature below 0.15 (vanilla's snow threshold, which is how 1.18's biomes set it).
    fn snowy(&self) -> bool {
        self.has_precipitation && self.temperature < 0.15
    }

    /// `BiomeUtil.getType`: the first of the badlands/desert-village, savanna, jungle, taiga (snowy → tundra) and
    /// forest (snowy → tundra, downfall ≥ 0.8 → temperate rainforest) tags, else grassland. A biome with none of
    /// these tags, a mod biome's usual case, is grassland whatever its climate.
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
            BiomeType::Grassland
        }
    }
}

/// `type.name().toLowerCase(Locale.ROOT)`: the path of the climate file `terraforged:<name>` (`BiomeTypeHolder`).
fn climate_name(climate: BiomeType) -> &'static str {
    match climate {
        BiomeType::TropicalRainforest => "tropical_rainforest",
        BiomeType::Savanna => "savanna",
        BiomeType::Desert => "desert",
        BiomeType::TemperateRainforest => "temperate_rainforest",
        BiomeType::TemperateForest => "temperate_forest",
        BiomeType::Grassland => "grassland",
        BiomeType::ColdSteppe => "cold_steppe",
        BiomeType::Steppe => "steppe",
        BiomeType::Taiga => "taiga",
        BiomeType::Tundra => "tundra",
        BiomeType::Alpine => "alpine",
    }
}

/// `Identifier.parse`'s split: `(namespace, path)`, the namespace `minecraft` when there is no `:` or nothing
/// before it (`:plains` is `minecraft:plains`, as `Identifier.parse` and 1.18's `new ResourceLocation` read it).
pub(super) fn identifier(name: &str) -> (&str, &str) {
    match name.split_once(':') {
        Some(("", path)) => ("minecraft", path),
        Some(split) => split,
        None => ("minecraft", name),
    }
}

/// `BiomeUtil.BIOME_SORTER` (`Identifier.compareTo`): the path first, then the namespace, each by
/// `String.compareTo` (UTF-16 code units).
pub(super) fn identifier_order(left: &str, right: &str) -> Ordering {
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
/// `minecraft:plains`), the later key's weight wins. The pool takes each file's biomes in `ClimateType`'s hash-map
/// order, not file order (see [`RegistryBiomeMap::build`]). TerraForged's own climate files are all empty (`{}`).
///
/// Not represented: keys `Identifier.parse` rejects (the codec throws; whoever decodes the files must refuse them),
/// and files named after no climate type (`terraforged:foo`): Java adds their biomes to the overworld list (and throws
/// on an unknown one) but gives them no pool.
#[derive(Clone)]
pub struct ClimateWeight {
    pub climate: BiomeType,
    pub biome: Arc<str>,
    pub weight: f32,
}

/// `BiomeMapManager`: the overworld biome list (`getOverworldBiomes`, `Source.possibleBiomes`) and one weighted
/// pool per climate (`getBiomeMap`).
pub struct RegistryBiomeMap<T> {
    pub overworld: Vec<Arc<T>>,
    pub pools: BiomePools<T>,
}

impl<T> RegistryBiomeMap<T> {
    /// `new BiomeMapManager(access)` over `entries` (the biome registry, in any order: nothing below depends on it)
    /// and `configured` (the climate files).
    ///
    /// - `overworld`: every `minecraft:is_overworld` biome plus every biome a climate file names, sorted by
    ///   `BIOME_SORTER` (path, then namespace).
    /// - `pools`: every climate type has a pool. First its file's biomes with their weights, in the file's hash-map
    ///   order; then, in `overworld` order at weight 1, every biome no file names, in its [`climate_type`] (untagged
    ///   biomes are grassland). Ocean, river, beach and cave biomes are not filtered out, and an empty pool stays
    ///   empty: no climate borrows another's biomes.
    ///
    /// `Err` with the first key (`namespace:path`) a climate file names that the registry lacks, where Java's
    /// `getHolderOrThrow` throws: files in the climate registry's order (26.3 loads data-driven registries in
    /// `Identifier` order, so `terraforged:<type>` by type name), each in its hash-map order.
    ///
    /// [`climate_type`]: RegistryBiome::climate_type
    pub fn build(
        entries: &[RegistryBiome<T>],
        configured: &[ClimateWeight],
    ) -> Result<Self, Arc<str>> {
        Self::build_with(entries, configured, false)
    }

    /// `build`; with `corrected` (the `corrected_biomes` setting) the unconfigured cave,
    /// ocean, river and beach biomes stay out of the pools. They stay in `overworld`,
    /// the biomes structures may find.
    pub fn build_with(
        entries: &[RegistryBiome<T>],
        configured: &[ClimateWeight],
        corrected: bool,
    ) -> Result<Self, Arc<str>> {
        let find = |name: &str| {
            let key = identifier(name);
            entries.iter().find(|entry| identifier(&entry.name) == key)
        };
        // Each file as `ClimateType`'s `Object2FloatOpenHashMap<Identifier>` (one entry per key, the last weight),
        // in its iteration order.
        let files: [Vec<(&str, f32)>; 11] = std::array::from_fn(|climate| {
            let mut file: Vec<(&str, f32)> = Vec::new();
            for assignment in configured
                .iter()
                .filter(|a| a.climate as usize == climate && a.biome.as_ref() != CLIMATE_IGNORE)
            {
                let key = identifier(&assignment.biome);
                match file.iter_mut().find(|(name, _)| identifier(name) == key) {
                    Some(existing) => existing.1 = assignment.weight,
                    None => file.push((&assignment.biome, assignment.weight)),
                }
            }
            let hashes: Vec<_> = file.iter().map(|(name, _)| identifier_hash(name)).collect();
            open_hash_order(&hashes)
                .into_iter()
                .map(|index| file[index])
                .collect()
        });

        // `getOverworldBiomes`: the tagged biomes, then each file's biomes (climate registry order), then the sort.
        let mut overworld: Vec<_> = entries
            .iter()
            .filter(|entry| entry.is(IS_OVERWORLD))
            .collect();
        let mut registry_order = BiomeType::VALUES;
        registry_order.sort_by_key(|climate| climate_name(*climate));
        let mut resolved: [Vec<(&RegistryBiome<T>, f32)>; 11] = std::array::from_fn(|_| Vec::new());
        for climate in registry_order {
            for &(name, weight) in &files[climate as usize] {
                let (namespace, path) = identifier(name);
                let entry = find(name).ok_or_else(|| Arc::from(format!("{namespace}:{path}")))?;
                resolved[climate as usize].push((entry, weight));
                if !overworld
                    .iter()
                    .any(|known| Arc::ptr_eq(&known.value, &entry.value))
                {
                    overworld.push(entry);
                }
            }
        }
        overworld.sort_by(|left, right| identifier_order(&left.name, &right.name));

        // `getWeightsMap`.
        let mut registered = BTreeSet::new();
        let mut weighted: [Vec<(Arc<T>, f32)>; 11] = std::array::from_fn(|_| Vec::new());
        for climate in BiomeType::VALUES {
            for &(entry, weight) in &resolved[climate as usize] {
                weighted[climate as usize].push((Arc::clone(&entry.value), weight));
                registered.insert(Arc::clone(&entry.name));
            }
        }
        for entry in &overworld {
            if !registered.contains(&entry.name) && !(corrected && !belongs_on_land(entry)) {
                weighted[entry.climate_type() as usize].push((Arc::clone(&entry.value), 1.0));
            }
        }

        // `buildBiomeMap`.
        let mut pools = BiomePools::default();
        for climate in BiomeType::VALUES {
            let (values, weights): (Vec<_>, Vec<_>) =
                std::mem::take(&mut weighted[climate as usize])
                    .into_iter()
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
    /// [`RegistryBiomeMap::build`] over Horizon's loaded biome registry ([`registry_entries`]).
    pub fn from_generator_data(
        data: &GeneratorData,
        configured: &[ClimateWeight],
    ) -> Result<Self, Arc<str>> {
        Self::build(&registry_entries(data), configured)
    }
}

/// Horizon's loaded biome registry as [`RegistryBiome`]s: one per biome id, with its climate settings and the
/// (resolved) biome tags that list it.
pub(super) fn registry_entries(data: &GeneratorData) -> Vec<RegistryBiome<BiomeId>> {
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

    fn spec<T>(value: T, name: &str, t: f32, d: f32, p: bool, tags: &[&str]) -> RegistryBiome<T> {
        let tags = tags.iter().map(|tag| Arc::<str>::from(*tag));
        RegistryBiome::new(value, name, t, d, p, tags)
    }

    fn names<T: Clone>(map: &RegistryBiomeMap<T>, climate: BiomeType) -> Vec<T> {
        let pool = map.pools.get(climate).unwrap();
        pool.values()
            .iter()
            .map(|value| (**value).clone())
            .collect()
    }

    #[test]
    fn pools_share_one_arc_per_biome() {
        let forest = [IS_OVERWORLD, IS_FOREST];
        let entries = [
            spec("swamp", "minecraft:swamp", 0.8, 0.9, true, &[IS_OVERWORLD]),
            spec("dark", "minecraft:dark_forest", 0.7, 0.9, true, &forest),
        ];
        let configured = [ClimateWeight {
            climate: BiomeType::Desert,
            biome: Arc::from("swamp"),
            weight: 2.0,
        }];
        let map = RegistryBiomeMap::build(&entries, &configured).unwrap();
        let desert = map.pools.get(BiomeType::Desert).unwrap();
        let rainforest = map.pools.get(BiomeType::TemperateRainforest).unwrap();
        assert!(Arc::ptr_eq(&desert.values()[0], &entries[0].value));
        assert!(Arc::ptr_eq(&rainforest.values()[0], &entries[1].value));
        assert!(Arc::ptr_eq(&map.overworld[0], &entries[1].value));
        assert!(Arc::ptr_eq(&map.overworld[1], &entries[0].value));
    }

    /// 6dd607eb's `getType` reads only tags: a mod biome with none of them is grassland (weight 1), whatever its
    /// temperature and downfall, and a biome without `minecraft:is_overworld` is in no pool unless a climate file
    /// names it. Oceans and caves stay in the pools, and empty climates stay empty.
    #[test]
    fn untagged_mod_biomes_are_grassland_and_nothing_is_filtered() {
        let named = |name: &str, t, d, tags: &[&str]| spec(name.to_owned(), name, t, d, true, tags);
        let entries = [
            named("mod:frozen_waste", -1.0, 0.0, &[IS_OVERWORLD]),
            named("mod:hot_jungle", 2.0, 1.0, &[IS_OVERWORLD]),
            named("mod:not_overworld", 0.8, 0.4, &[]),
            named(
                "minecraft:ocean",
                0.5,
                0.5,
                &[IS_OVERWORLD, "minecraft:is_ocean"],
            ),
            named("minecraft:lush_caves", 0.5, 0.5, &[IS_OVERWORLD]),
            named("mod:snowy_wood", 0.0, 0.5, &[IS_OVERWORLD, IS_FOREST]),
        ];
        let map = RegistryBiomeMap::build(&entries, &[]).unwrap();
        assert_eq!(
            names(&map, BiomeType::Grassland),
            [
                "mod:frozen_waste",
                "mod:hot_jungle",
                "minecraft:lush_caves",
                "minecraft:ocean"
            ]
        );
        assert_eq!(names(&map, BiomeType::Tundra), ["mod:snowy_wood"]);
        for climate in [BiomeType::Desert, BiomeType::Alpine, BiomeType::Steppe] {
            assert!(map.pools.get(climate).unwrap().is_empty());
        }
        assert_eq!(map.overworld.len(), 5);
    }

    #[test]
    fn identifiers_parse_like_java() {
        assert_eq!(identifier("plains"), ("minecraft", "plains"));
        assert_eq!(identifier(":plains"), ("minecraft", "plains"));
        assert_eq!(identifier("a:b"), ("a", "b"));
        assert_eq!(identifier("minecraft:"), ("minecraft", ""));
    }

    // ---- BiomeMapManagerOracle (`.local/terraforged/oracles/biome_registry/6dd607eb/`): 6dd607eb's real
    // BiomeMapManager over real 26.3 registries ----

    /// The oracle's `H` lines: (section, lines, FNV-1a 64 of the section's lines).
    const JAVA: [(&str, usize, u64); 7] = [
        ("vanilla", 13, 0x2a9d_716a_29c1_de20),
        ("terraforged", 13, 0xbf2b_3444_bf56_4990),
        ("modded", 13, 0xc970_8345_b412_5596),
        ("backwards", 13, 0xc970_8345_b412_5596),
        ("files", 13, 0x3eda_3296_c77e_8bf5),
        ("unknown", 8, 0x9ef5_fac6_822c_cd0f),
        ("random", 30_354, 0xdde3_2f86_1b67_4819),
    ];
    const RANDOM_CASES: u64 = 3000;

    struct Section(usize, u64);
    impl Section {
        fn new() -> Self {
            Self(0, 0xcbf2_9ce4_8422_2325)
        }
        fn emit(&mut self, line: String) {
            for byte in line.bytes().chain([b'\n']) {
                self.1 = (self.1 ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
            }
            self.0 += 1;
        }
        fn check(&self, section: &str) {
            let java = JAVA.iter().find(|(name, ..)| *name == section).unwrap();
            assert_eq!((self.0, self.1), (java.1, java.2), "{section}");
        }
        fn case<T>(
            &mut self,
            case: &str,
            map: Result<RegistryBiomeMap<T>, Arc<str>>,
            name: impl Fn(&T) -> String,
        ) {
            self.emit(format!("C {case}"));
            let map = match map {
                Ok(map) => map,
                Err(key) => return self.emit(format!("X {key}")),
            };
            let mut line = String::from("O");
            for value in &map.overworld {
                line = line + " " + &name(value);
            }
            self.emit(line);
            for climate in BiomeType::VALUES {
                let pool = map.pools.get(climate).unwrap();
                let mut line = format!("P {}", climate as usize);
                for value in pool.values() {
                    line += &format!(" {}={:x}", name(value), pool.band(value) as u64);
                }
                self.emit(line);
            }
        }
    }

    /// The oracle's `MODS`: name, temperature, downfall, precipitation, tags (o overworld, f forest, t taiga,
    /// j jungle, c ocean, h `terraforged:highland`, w `terraforged:wetland`: tags 6dd607eb doesn't read).
    const MODS: &str = "terralith:alpine_grove -0.2 0.5 1 o, terralith:sandstone_valley 1.5 0.0 0 o, \
        terralith:cave/frostfire_caves 0.0 0.5 1 o, terralith:moonlight_grove 0.6 0.85 1 of, \
        terralith:skylands 0.5 0.5 1 oh, terralith:yellowstone 0.9 0.4 1 o, biomesoplenty:bayou 0.8 0.95 1 ow, \
        biomesoplenty:tundra -0.3 0.3 1 o, biomesoplenty:cold_desert 0.0 0.0 0 o, biomesoplenty:lagoon 0.5 0.5 1 oc, \
        biomesoplenty:crag 0.3 0.2 1 ot, biomesoplenty:snowy_crag 0.1 0.2 1 ot, biomesoplenty:not_overworld 0.8 0.4 1 -, \
        zz:jungle_x 0.9 0.9 1 oj, a:plains 0.8 0.4 1 o, untagged:meadow 0.5 0.8 1 -";

    fn mods() -> impl Iterator<Item = (&'static str, f32, f32, bool, Vec<&'static str>)> {
        let tag = |code| match code {
            'o' => IS_OVERWORLD,
            'f' => IS_FOREST,
            't' => IS_TAIGA,
            'j' => IS_JUNGLE,
            'c' => "minecraft:is_ocean",
            'h' => "terraforged:highland",
            'w' => "terraforged:wetland",
            _ => unreachable!(),
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

    /// The oracle's `files`: desert (bare, `:`-prefixed, ignored, non-overworld and untagged keys, one biome under
    /// three keys), a 30-entry grassland (the hash map grows), a one-entry tundra file and an empty taiga file.
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
            (CLIMATE_IGNORE, 7.0),
            ("biomesoplenty:not_overworld", 0.5),
            (":plains", 2.0),
            ("minecraft:plains", 4.0),
            ("untagged:meadow", 1.5),
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
    fn vanilla_and_modded_registries_match_6dd607eb_java() {
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

        let mut section = Section::new();
        section.case(
            "vanilla",
            RegistryBiomeMap::from_generator_data(&data, &[]),
            name,
        );
        section.check("vanilla");
        // TerraForged's own climate files are all `{}`: the same pools.
        let mut section = Section::new();
        section.case("terraforged", RegistryBiomeMap::build(&vanilla, &[]), name);
        section.check("terraforged");
        let mut section = Section::new();
        section.case("modded", RegistryBiomeMap::build(&modded, &[]), name);
        section.check("modded");
        let mut backwards = modded.clone();
        backwards.reverse();
        let mut section = Section::new();
        section.case("modded", RegistryBiomeMap::build(&backwards, &[]), name);
        section.check("backwards");
        let mut section = Section::new();
        section.case("files", RegistryBiomeMap::build(&modded, &files()), name);
        section.check("files");

        let unknown: [&[(BiomeType, &str)]; 4] = [
            &[(BiomeType::Desert, "missingmod:foo")],
            &[
                (BiomeType::Desert, "missingmod:foo"),
                (BiomeType::Alpine, "zz:missing"),
            ],
            &[(BiomeType::Tundra, "a:x"), (BiomeType::ColdSteppe, "b:y")],
            &[(BiomeType::Grassland, ":nope")],
        ];
        let mut section = Section::new();
        for (index, extra) in unknown.iter().enumerate() {
            let mut configured = files();
            for &(climate, biome) in *extra {
                configured.extend(weights(climate, &[(biome, 1.0)]));
            }
            let map = RegistryBiomeMap::build(&modded, &configured);
            section.case(&format!("unknown{index}"), map, name);
        }
        section.check("unknown");
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
    fn random_registries_match_6dd607eb_java() {
        let ns: Vec<_> = "minecraft terralith biomesoplenty a zz terraforged"
            .split(' ')
            .collect();
        let paths: Vec<_> = "plains forest cave/deep lush_caves ocean stony_shore swamp windswept_hills badlands a \
            b/c z plains_caves river beach snowy_taiga jungle savanna mangrove_swamp grove deep_dark cave alpine \
            x_caves_y a-b a.b a_b ab cave/ caves"
            .split_whitespace()
            .collect();
        let tags_pool = [
            IS_BADLANDS,
            HAS_VILLAGE_DESERT,
            IS_SAVANNA,
            IS_JUNGLE,
            IS_TAIGA,
            IS_FOREST,
            "minecraft:is_ocean",
            "terraforged:non_surface",
            "minecraft:other",
        ];
        // Java's `Float.parseFloat` of the oracle's literals.
        let parse =
            |text: &str| -> Vec<f32> { text.split(' ').map(|v| v.parse().unwrap()).collect() };
        let floats = parse(
            "-2.0 -0.7 -0.5 0.0 -0.0 0.1 0.14999999 0.15 0.15000001 0.2 0.25 0.3 0.4 0.5 0.6 0.7 0.79999995 \
             0.8 0.8000001 0.9 0.95 1.0 1.2 1.5 2.0 NaN Infinity -Infinity 1.4E-45",
        );
        let weights = parse("1 5 0 0.5 3 2.5 NaN 1.4E-45 100 -1");
        let mut section = Section::new();
        for case in 0..RANDOM_CASES {
            let mut g = Gen(case.wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(1));
            let name =
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
            let mut configured: Vec<ClimateWeight> = Vec::new();
            for climate in BiomeType::VALUES {
                let count = match g.next(5) {
                    0 | 1 => 0,
                    4 => 20 + g.next(25),
                    _ => 1 + g.next(6),
                };
                if count == 0 && g.next(2) == 0 {
                    continue; // no file at all: the same as an empty one
                }
                let file = configured.len();
                for _ in 0..count {
                    let pick = g.next(16);
                    let biome = if pick == 0 || specs.is_empty() {
                        CLIMATE_IGNORE.to_owned()
                    } else if pick == 1 && g.next(16) == 0 {
                        name(&mut g)
                    } else {
                        let biome = specs[g.next(specs.len())].name.to_string();
                        match biome.strip_prefix("minecraft:") {
                            Some(path) => match g.next(3) {
                                0 => path.to_owned(),
                                1 => format!(":{path}"),
                                _ => biome,
                            },
                            None => biome,
                        }
                    };
                    let weight = weights[g.next(weights.len())];
                    // One entry per JSON key (`JsonObject.has`).
                    if !configured[file..].iter().any(|w| *w.biome == *biome) {
                        configured.push(ClimateWeight {
                            climate,
                            biome: Arc::from(biome),
                            weight,
                        });
                    }
                }
            }
            let map = RegistryBiomeMap::build(&specs, &configured);
            section.case(&case.to_string(), map, String::clone);
        }
        section.check("random");
    }

    #[test]
    fn corrected_pools_keep_caves_and_water_off_land() {
        let biome = |name: &'static str, tags: &[&str]| spec(name, name, 0.8, 0.4, true, tags);
        let entries = [
            biome("minecraft:plains", &[IS_OVERWORLD]),
            biome("minecraft:deep_dark", &[IS_OVERWORLD]),
            biome(
                "minecraft:deep_ocean",
                &[
                    IS_OVERWORLD,
                    "minecraft:is_ocean",
                    "minecraft:is_deep_ocean",
                ],
            ),
            biome("minecraft:river", &[IS_OVERWORLD, "minecraft:is_river"]),
            biome("minecraft:beach", &[IS_OVERWORLD, "minecraft:is_beach"]),
        ];
        let pooled = |corrected| {
            let map = RegistryBiomeMap::build_with(&entries, &[], corrected).unwrap();
            let mut pooled: Vec<&str> = BiomeType::VALUES
                .iter()
                .flat_map(|climate| names(&map, *climate))
                .collect();
            pooled.sort_unstable();
            (pooled, map.overworld.len())
        };
        assert_eq!(pooled(false).0.len(), 5);
        assert_eq!(pooled(true), (vec!["minecraft:plains"], 5));
    }
}
