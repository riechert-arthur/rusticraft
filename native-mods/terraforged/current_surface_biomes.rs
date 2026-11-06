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
// `src/main/java/com/terraforged/mod/worldgen/biome/BiomeSampler.java`, `sampleBiome` (getSample, then
// getInitialBiome, then getBiomeOverride). TerraForged 6dd607eb's `sampleBiome` has the same three steps over one
// climate pool. The override itself is 6dd607eb's `getBiomeOverride` with its 0.005 beach band (`biome_override`),
// not Reimagined's `COAST - BEACH` band. MIT License, Copyright (c) 2020 TerraForged, Copyright (c) 2026
// Romoslayer; see `LICENSE-TERRAFORGED-REIMAGINED`.

use super::{
    terraforged_biome_map::BiomePools,
    terraforged_biome_sampler::{
        BiomeOverride, ClimateSample, InitialBiomeSelector, WetlandSettings, biome_override,
    },
    terraforged_current_biome_sampler::CurrentBiomeSampler,
    terraforged_noise_generator::NoiseGenerator,
};
use std::sync::Arc;

/// Reimagined's `BiomeSampler` as `Source.compute` uses it: the surface biome of a block column. It joins the
/// climate sample ([`CurrentBiomeSampler`]), the initial biome ([`InitialBiomeSelector`]: wet or dry pool, then the
/// highland swap) and 6dd607eb's surface override ([`biome_override`], which wins). The height noise is
/// the generator's un-eroded `getHeightNoise` (Reimagined's `ErodedNoiseGenerator` delegates it to the raw
/// generator), the levels its `TerrainLevels`. `wetlands` and `mountain_usage` are Reimagined's
/// `TerraSettings.rivers.wetlands` and `miscellaneous.mountainBiomeUsage`. Biomes are compared by identity, so the
/// three pool sets must share one `Arc` per biome. Not ported: `reportSlope` (a debug-log survey) and `Source`'s
/// per-column cache.
pub struct CurrentSurfaceBiomeSampler<'a, T> {
    climate: CurrentBiomeSampler<'a>,
    noise: &'a NoiseGenerator,
    initial: InitialBiomeSelector<'a, T>,
}

// TerraForged samples biomes from its worker threads.
const _: fn() = || {
    fn shareable<T: Send + Sync>() {}
    shareable::<CurrentSurfaceBiomeSampler<'static, String>>();
};

impl<'a, T> CurrentSurfaceBiomeSampler<'a, T> {
    /// `new BiomeSampler(noiseGenerator, biomeMapManager)`: the climate sampler (its seeds drawn as
    /// [`CurrentBiomeSampler::new`] does), the manager's wet (`getBiomeMap`), dry and highland pools and its plains
    /// biome, and the generator's settings and levels. Building the climate noise is not cheap: build one sampler
    /// per world and share it across threads (TerraForged builds one, in `Source`), not one per chunk.
    pub fn new(
        noise: &'a NoiseGenerator,
        wet: &'a BiomePools<T>,
        dry: &'a BiomePools<T>,
        highland: &'a BiomePools<T>,
        plains: &'a Arc<T>,
        wetlands: WetlandSettings,
        mountain_usage: f32,
    ) -> Self {
        Self {
            climate: CurrentBiomeSampler::new(noise),
            noise,
            initial: InitialBiomeSelector {
                wet,
                dry,
                highland,
                plains,
                wetlands,
                mountain_usage,
                levels: noise.levels(),
            },
        }
    }

    /// The generator whose noise the sampler reads (the climate's continent and the height noise).
    pub const fn noise(&self) -> &'a NoiseGenerator {
        self.noise
    }

    /// `sampleBiome(seed, x, z)` at block coordinates (`Source.compute` passes quart `x << 2`, `z << 2`) for the
    /// runtime seed. `base_temperature` is a biome's `getBaseTemperature`; `override_biome` resolves a surface
    /// override (`biomeMapManager.get(key)`, see [`BiomeOverride::key`]) and is never called with
    /// [`BiomeOverride::Input`]. Fails with the height noise's first error (a custom module, where Java throws).
    pub fn sample_biome(
        &self,
        seed: i32,
        x: i32,
        z: i32,
        base_temperature: impl Fn(&T) -> f32,
        override_biome: impl Fn(BiomeOverride) -> Arc<T>,
    ) -> Result<Arc<T>, &'static str> {
        let sample = self.climate.get_sample(seed, x, z);
        surface_biome(
            &self.initial,
            &sample,
            x,
            z,
            |x, z| self.noise.height_noise(seed, x, z),
            base_temperature,
            override_biome,
        )
    }
}

/// `sampleBiome` after `getSample`: [`InitialBiomeSelector::initial_biome`] first (its height reads and their
/// error come even where the override then replaces the biome, as in Java), then [`biome_override`]; a
/// replacement goes through `override_biome`.
pub fn surface_biome<T, E>(
    initial: &InitialBiomeSelector<'_, T>,
    sample: &ClimateSample,
    x: i32,
    z: i32,
    height_noise: impl FnMut(i32, i32) -> Result<f32, E>,
    base_temperature: impl Fn(&T) -> f32,
    override_biome: impl Fn(BiomeOverride) -> Arc<T>,
) -> Result<Arc<T>, E> {
    let biome = initial.initial_biome(sample, x, z, height_noise, base_temperature)?;
    Ok(match biome_override(sample) {
        BiomeOverride::Input => biome,
        replacement => override_biome(replacement),
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::super::{
        terraforged_biome_sampler::PLAINS,
        terraforged_current_biome_sampler::tests::{generator, points},
        terraforged_engine::{BiomeType, TerrainRegistry},
        terraforged_terrain_blend_util::WeightMap,
        terraforged_terrain_levels::TerrainLevels,
    };
    use super::*;
    use std::collections::BTreeMap;

    pub(crate) const WETLANDS: WetlandSettings = WetlandSettings {
        river_size: 0.75,
        coast_size: 0.7,
        chance: 1.0,
    };

    fn one(biome: &Arc<&'static str>) -> BiomePools<&'static str> {
        let mut pools = BiomePools::default();
        for climate in BiomeType::VALUES {
            pools.insert(
                climate,
                Arc::new(WeightMap::new(vec![Arc::clone(biome)], &[1.0])),
            );
        }
        pools
    }

    fn named(replacement: BiomeOverride) -> Arc<&'static str> {
        Arc::new(replacement.key().unwrap())
    }

    /// A steep land column (west 0, east 1 at `maxY` 480: gradient 20) with snow-compatible biomes.
    fn steep(x: i32, _z: i32) -> Result<f32, &'static str> {
        Ok(if x > 0 { 1.0 } else { 0.0 })
    }

    #[test]
    fn override_wins_after_the_initial_biome() {
        let (wet, dry, highland) = (Arc::new("wet"), Arc::new("dry"), Arc::new("highland"));
        let (wet, dry, highland) = (one(&wet), one(&dry), one(&highland));
        let plains = Arc::new(PLAINS);
        let selector = InitialBiomeSelector {
            wet: &wet,
            dry: &dry,
            highland: &highland,
            plains: &plains,
            wetlands: WETLANDS,
            mountain_usage: 1.0,
            levels: TerrainLevels::default(),
        };
        let terrains = TerrainRegistry::new();
        let mut sample = ClimateSample::new(&terrains);
        sample.climate_type = BiomeType::Grassland;
        sample.biome_noise = 0.4;
        sample.noise.river_noise = 1.0;
        let biome = |sample: &ClimateSample| {
            surface_biome(&selector, sample, 0, 0, steep, |_| 0.8, named).unwrap()
        };

        sample.noise.continent_noise = 0.9;
        assert_eq!(*biome(&sample), "highland");
        // 6dd607eb's 0.005 beach band stops at 0.505f; Reimagined's wider one (up to COAST, 0.55) is not followed.
        sample.noise.continent_noise = 0.505;
        assert_eq!(*biome(&sample), "minecraft:beach");
        sample.noise.continent_noise = 0.53;
        assert_eq!(*biome(&sample), "highland");
        sample.noise.continent_noise = 0.1;
        assert_eq!(*biome(&sample), "minecraft:deep_ocean");
        sample.noise.continent_noise = 0.9;
        sample.noise.river_noise = 0.0;
        sample.noise.terrain_type = terrains.get_by_name("river").unwrap();
        assert_eq!(*biome(&sample), "minecraft:river");

        // Java reads the height noise in getInitialBiome before getBiomeOverride, so its failure surfaces even in
        // the ocean.
        sample.noise.continent_noise = 0.1;
        let mut reads = 0;
        let failed = surface_biome(
            &selector,
            &sample,
            0,
            0,
            |_, _| {
                reads += 1;
                Err::<f32, _>("custom module")
            },
            |_| 0.8,
            named,
        );
        assert_eq!(failed.err(), Some("custom module"));
        assert_eq!(reads, 1);
    }

    /// `.local/terraforged/oracles/current_surface_biomes/` (`run.sh`, after `../noise_generator/run.sh`): half A
    /// records what `sampleBiome` reads from 6dd607eb's real `Sampler` and newer-source `NoiseGenerator` (the
    /// continent, rivers and the five height reads of each column); half B runs Reimagined a24d450's real
    /// `BiomeSampler.sampleBiome` (its real `Sampler`, `ClimateNoise` and pre-increment `Seed`; 6dd607eb's 0.005
    /// `beachSize` swapped in by `biome_override/6dd607eb/patch_beach.py`) over that record, interpreted and with
    /// `-Xcomp`. Per levels variant, terrain set and pool configuration ([`pools`]): the
    /// column count and an FNV-1a fold of the biome names over the six runtime seeds in order; then every name's
    /// count (oceans, beaches, rivers, the plains fallback and every pool biome appear).
    const EXPECTED: &str = "\
biomes 0 0 0 1212 e169326726e5f533\n\
biomes 0 0 1 1212 3c5be5db782ae25\n\
biomes 0 0 2 1212 b05c9801794c685a\n\
biomes 0 0 3 1212 10d1069aa09c71c5\n\
biomes 0 1 0 1212 97a92a236e7dfffa\n\
biomes 0 1 1 1212 dd6a599396d23356\n\
biomes 0 1 2 1212 a1d6af3cac448cad\n\
biomes 0 1 3 1212 e46c309e702ec844\n\
biomes 1 0 0 1212 2fcc69c43bb9a080\n\
biomes 1 0 1 1212 a1d1c316e43326f5\n\
biomes 1 0 2 1212 b7c7328c1fbe1b9e\n\
biomes 1 0 3 1212 eee4c2ac6bae0361\n\
biomes 1 1 0 1212 2d5dbf074386b17\n\
biomes 1 1 1 1212 46d0b8a254589b99\n\
biomes 1 1 2 1212 a63a9f7643ba8a9d\n\
biomes 1 1 3 1212 eee0bca9e8f42734\n\
biomes 2 0 0 1212 62c800b424eac338\n\
biomes 2 0 1 1212 a723e75ed8206589\n\
biomes 2 0 2 1212 3c3eb8dc125c4de0\n\
biomes 2 0 3 1212 213019c18b26a571\n\
biomes 2 1 0 1212 9f14072d01f15dca\n\
biomes 2 1 1 1212 803bf72a44c1a915\n\
biomes 2 1 2 1212 dd508c771a26f67d\n\
biomes 2 1 3 1212 cabaf8634bc782f\n\
names birch_forest=558 desert=811 forest=834 grove=796 jungle=1219 meadow=902 minecraft:beach=80 minecraft:cold_ocean=96 minecraft:deep_cold_ocean=1928 minecraft:deep_frozen_ocean=2664 minecraft:deep_lukewarm_ocean=5120 minecraft:deep_ocean=6808 minecraft:frozen_ocean=160 minecraft:frozen_river=200 minecraft:ocean=168 minecraft:river=1952 minecraft:snowy_beach=32 minecraft:warm_ocean=120 plains=718 snowy_plains=925 swamp=1269 taiga=796 windswept_hills=932\n";

    pub(crate) const NAMES: [&str; 10] = [
        "forest",
        "birch_forest",
        "taiga",
        "snowy_plains",
        "desert",
        "windswept_hills",
        "swamp",
        "jungle",
        "meadow",
        "grove",
    ];
    pub(crate) const TEMPS: [f32; 10] = [0.7, 0.6, 0.25, 0.0, 2.0, 0.2, 0.8, 0.95, 0.5, -0.2];
    pub(crate) const USAGE: [f32; 4] = [1.0, 0.5, 0.75, 1.0];
    pub(crate) const CHANCE: [f32; 4] = [1.0, 0.5, 1.0, 0.25];
    const SEEDS: [i32; 6] = [0, 42, -17, i32::MAX, i32::MIN, -8_763_214];

    /// The oracle's `pools(manager, k)`: per climate type `c`, a wet pool of two biomes (weights 1 and 2; empty for
    /// `k == 3` and `c % 4 == 1`), a dry pool of one (absent for `k == 1` and `c % 3 == 0`) and a highland pool of
    /// two (absent for `k == 2` and even `c`), whose second biome is the wet pool's first, all over one shared `Arc`
    /// per biome.
    pub(crate) fn pools<T>(palette: &[Arc<T>], k: usize) -> [BiomePools<T>; 3] {
        let map = |weights: &[f32], index: &[usize]| {
            let values = index.iter().map(|&i| Arc::clone(&palette[i])).collect();
            Arc::new(WeightMap::new(values, weights))
        };
        let (mut wet, mut dry, mut highland) = Default::default();
        let [wet_pools, dry_pools, highland_pools]: [&mut BiomePools<T>; 3] =
            [&mut wet, &mut dry, &mut highland];
        for climate in BiomeType::VALUES {
            let c = climate as usize;
            if k == 3 && c % 4 == 1 {
                wet_pools.insert(climate, map(&[], &[]));
            } else {
                wet_pools.insert(
                    climate,
                    map(&[1.0, 2.0], &[(c + k) % 10, (c + 2 * k + 3) % 10]),
                );
            }
            if !(k == 1 && c % 3 == 0) {
                dry_pools.insert(climate, map(&[1.0], &[(c + 1 + k) % 10]));
            }
            if !(k == 2 && c % 2 == 0) {
                highland_pools.insert(climate, map(&[1.0, 1.0], &[(c + 5) % 10, (c + k) % 10]));
            }
        }
        [wet, dry, highland]
    }

    fn lines() -> Vec<String> {
        let palette: Vec<Arc<String>> = NAMES
            .iter()
            .map(|name| Arc::new(name.to_string()))
            .collect();
        let plains = Arc::new("plains".to_string());
        let temperature = |biome: &String| {
            NAMES
                .iter()
                .position(|name| name == biome)
                .map_or(0.8, |i| TEMPS[i])
        };
        let replace = |replacement: BiomeOverride| Arc::new(replacement.key().unwrap().to_string());
        let configs: Vec<_> = (0..4).map(|k| pools(&palette, k)).collect();
        let mut counts = BTreeMap::new();
        let mut lines = Vec::new();
        for variant in 0..3 {
            for set in 0..2 {
                let mut folds = [(0, 0xcbf2_9ce4_8422_2325_u64); 4];
                for seed in SEEDS {
                    let generator = generator(variant, set);
                    for (k, [wet, dry, highland]) in configs.iter().enumerate() {
                        let wetlands = WetlandSettings {
                            chance: CHANCE[k],
                            ..WETLANDS
                        };
                        let sampler = CurrentSurfaceBiomeSampler::new(
                            &generator, wet, dry, highland, &plains, wetlands, USAGE[k],
                        );
                        let (columns, hash) = &mut folds[k];
                        for (x, z) in points() {
                            let biome = sampler
                                .sample_biome(seed, x, z, temperature, replace)
                                .unwrap();
                            for byte in biome.bytes().chain([0xff]) {
                                *hash = (*hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
                            }
                            *columns += 1;
                            *counts.entry(biome.to_string()).or_insert(0) += 1;
                        }
                    }
                }
                for (k, (columns, hash)) in folds.iter().enumerate() {
                    lines.push(format!("biomes {variant} {set} {k} {columns} {hash:x}"));
                }
            }
        }
        let mut names = String::from("names");
        for (name, count) in counts {
            names.push_str(&format!(" {name}={count}"));
        }
        lines.push(names);
        lines
    }

    #[test]
    fn surface_biomes_match_reimagined_java() {
        let expected: Vec<&str> = EXPECTED.lines().collect();
        let actual = lines();
        assert_eq!(actual.len(), expected.len());
        for (actual, expected) in actual.iter().zip(expected) {
            assert_eq!(actual, expected);
        }
    }
}
