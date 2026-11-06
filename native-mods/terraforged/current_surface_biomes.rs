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

// TerraForged 6dd607eb (https://github.com/TerraForged/TerraForged/tree/6dd607ebfd41a3b7274e090b1f2cf565d53d1d44,
// MIT License, Copyright (c) 2021 TerraForged): `src/main/java/com/terraforged/mod/worldgen/biome/BiomeSampler.java`,
// `sampleBiome` (getSample, then getInitialBiome over one climate pool, then getBiomeOverride). TerraForged
// Reimagined's `sampleBiome` (wet/dry pools, highland swap, slope and snow checks) is not followed.

use super::{
    terraforged_biome_map::BiomePools,
    terraforged_biome_sampler::{BiomeOverride, ClimateSample, biome_override, initial_biome},
    terraforged_current_biome_sampler::CurrentBiomeSampler,
    terraforged_noise_generator::NoiseGenerator,
};
use std::sync::Arc;

/// 6dd607eb's `BiomeSampler` as `Source.compute` uses it: the surface biome of a block column. It joins the
/// climate sample ([`CurrentBiomeSampler`], with 6dd607eb's climate noise), the climate type's pool
/// ([`initial_biome`]) and the surface override ([`biome_override`], which wins). It reads no height noise. Not
/// ported: `Source`'s per-column cache.
pub struct CurrentSurfaceBiomeSampler<'a, T> {
    climate: CurrentBiomeSampler<'a>,
    noise: &'a NoiseGenerator,
    pools: &'a BiomePools<T>,
    plains: &'a Arc<T>,
}

// TerraForged samples biomes from its worker threads.
const _: fn() = || {
    fn shareable<T: Send + Sync>() {}
    shareable::<CurrentSurfaceBiomeSampler<'static, String>>();
};

/// The error of a column whose pool answers Java's null (see [`initial_biome`]) and that no override replaces: Java
/// returns that null from `sampleBiome` and fails later on it.
pub const NO_POOL_BIOME: &str = "WeightMap.getValue returned no biome for the column's noise";

impl<'a, T> CurrentSurfaceBiomeSampler<'a, T> {
    /// `new BiomeSampler(noiseGenerator, biomeMapManager)`: the climate sampler (its seeds drawn as
    /// [`CurrentBiomeSampler::new`] does), the manager's pools (`getBiomeMap`) and its plains biome. Building the
    /// climate noise is not cheap: build one sampler per world and share it across threads (TerraForged builds one,
    /// in `Source`), not one per chunk.
    pub fn new(noise: &'a NoiseGenerator, pools: &'a BiomePools<T>, plains: &'a Arc<T>) -> Self {
        Self {
            climate: CurrentBiomeSampler::new(noise),
            noise,
            pools,
            plains,
        }
    }

    /// The generator whose noise the sampler reads (the climate's continent and rivers).
    pub const fn noise(&self) -> &'a NoiseGenerator {
        self.noise
    }

    /// `sampleBiome(seed, x, z)` at block coordinates (`Source.compute` passes quart `x << 2`, `z << 2`) for the
    /// runtime seed. `override_biome` resolves a surface override (`biomeMapManager.get(key)`, see
    /// [`BiomeOverride::key`]) and is never called with [`BiomeOverride::Input`]. `Err` ([`NO_POOL_BIOME`]) where
    /// Java's `sampleBiome` returns null.
    pub fn sample_biome(
        &self,
        seed: i32,
        x: i32,
        z: i32,
        override_biome: impl Fn(BiomeOverride) -> Arc<T>,
    ) -> Result<Arc<T>, &'static str> {
        let sample = self.climate.get_sample(seed, x, z);
        surface_biome(self.pools, self.plains, &sample, override_biome)
    }
}

/// `sampleBiome` after `getSample`: [`initial_biome`], then [`biome_override`]; a replacement goes through
/// `override_biome` and wins, even over a pool that had no biome. `Err` ([`NO_POOL_BIOME`]) where Java's result is
/// null.
pub fn surface_biome<T>(
    pools: &BiomePools<T>,
    plains: &Arc<T>,
    sample: &ClimateSample,
    override_biome: impl Fn(BiomeOverride) -> Arc<T>,
) -> Result<Arc<T>, &'static str> {
    let biome = initial_biome(pools, sample, plains);
    match biome_override(sample) {
        BiomeOverride::Input => biome.ok_or(NO_POOL_BIOME),
        replacement => Ok(override_biome(replacement)),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::super::{
        terraforged_biome_sampler::PLAINS,
        terraforged_current_biome_sampler::tests::{generator, points},
        terraforged_engine::{BiomeType, TerrainRegistry},
        terraforged_terrain_blend_util::WeightMap,
    };
    use super::*;
    use std::collections::BTreeMap;

    fn one(biome: &Arc<&'static str>, weight: f32) -> BiomePools<&'static str> {
        let mut pools = BiomePools::default();
        for climate in BiomeType::VALUES {
            pools.insert(
                climate,
                Arc::new(WeightMap::new(vec![Arc::clone(biome)], &[weight])),
            );
        }
        pools
    }

    fn named(replacement: BiomeOverride) -> Arc<&'static str> {
        Arc::new(replacement.key().unwrap())
    }

    #[test]
    fn override_wins_after_the_initial_biome() {
        let land = Arc::new("land");
        let pools = one(&land, 1.0);
        let plains = Arc::new(PLAINS);
        let terrains = TerrainRegistry::new();
        let mut sample = ClimateSample::new(&terrains);
        sample.climate_type = BiomeType::Grassland;
        sample.biome_noise = 0.4;
        sample.noise.river_noise = 1.0;
        let biome = |pools: &BiomePools<&'static str>, sample: &ClimateSample| {
            surface_biome(pools, &plains, sample, named)
        };

        sample.noise.continent_noise = 0.9;
        assert_eq!(*biome(&pools, &sample).unwrap(), "land");
        // 6dd607eb's 0.005 beach band stops at 0.505f.
        sample.noise.continent_noise = 0.505;
        assert_eq!(*biome(&pools, &sample).unwrap(), "minecraft:beach");
        sample.noise.continent_noise = 0.53;
        assert_eq!(*biome(&pools, &sample).unwrap(), "land");
        sample.noise.continent_noise = 0.1;
        assert_eq!(*biome(&pools, &sample).unwrap(), "minecraft:deep_ocean");
        sample.noise.continent_noise = 0.9;
        sample.noise.river_noise = 0.0;
        sample.noise.terrain_type = terrains.get_by_name("river").unwrap();
        assert_eq!(*biome(&pools, &sample).unwrap(), "minecraft:river");

        // A pool whose `getValue` is null (all weights zero): Java's sampleBiome returns null unless an override
        // replaces it.
        let null = one(&land, 0.0);
        assert_eq!(*biome(&null, &sample).unwrap(), "minecraft:river");
        sample.noise.river_noise = 1.0;
        assert_eq!(biome(&null, &sample).err(), Some(NO_POOL_BIOME));
        // No pool for the climate type: plains.
        assert_eq!(
            *biome(&BiomePools::default(), &sample).unwrap(),
            "minecraft:plains"
        );
    }

    /// `.local/terraforged/oracles/current_surface_biomes/` (`run.sh`, after `../noise_generator/run.sh`): half A
    /// records what `sampleBiome` reads from 6dd607eb's real `Sampler` and newer-source `NoiseGenerator` (the
    /// continent and rivers of each column); half B runs TerraForged 6dd607eb's real `BiomeSampler.sampleBiome`
    /// over that record, with 6dd607eb's real `IBiomeSampler.Sampler` and `ClimateNoise` (Engine 0.3.0's
    /// post-increment `Seed`, `createContinentNoise`'s settings, from `../climate_noise`), interpreted and with
    /// `-Xcomp`. Per levels variant, terrain set and pool configuration ([`pools`]): the column count and an
    /// FNV-1a fold of the biome names over the six runtime seeds in order; then every name's count (oceans,
    /// beaches, rivers, the plains fallback, every pool biome and Java's null result, `null`, appear).
    const EXPECTED: &str = "\
biomes 0 0 0 1212 28bfd14e0dad82e0\n\
biomes 0 0 1 1212 f868b88643f205e9\n\
biomes 0 0 2 1212 a9fce8a89c381ec4\n\
biomes 0 0 3 1212 6aa7ef4ce1e8da62\n\
biomes 0 0 4 1212 a7e2afc134f5e814\n\
biomes 0 1 0 1212 28bfd14e0dad82e0\n\
biomes 0 1 1 1212 f868b88643f205e9\n\
biomes 0 1 2 1212 a9fce8a89c381ec4\n\
biomes 0 1 3 1212 6aa7ef4ce1e8da62\n\
biomes 0 1 4 1212 a7e2afc134f5e814\n\
biomes 1 0 0 1212 28bfd14e0dad82e0\n\
biomes 1 0 1 1212 f868b88643f205e9\n\
biomes 1 0 2 1212 a9fce8a89c381ec4\n\
biomes 1 0 3 1212 6aa7ef4ce1e8da62\n\
biomes 1 0 4 1212 a7e2afc134f5e814\n\
biomes 1 1 0 1212 28bfd14e0dad82e0\n\
biomes 1 1 1 1212 f868b88643f205e9\n\
biomes 1 1 2 1212 a9fce8a89c381ec4\n\
biomes 1 1 3 1212 6aa7ef4ce1e8da62\n\
biomes 1 1 4 1212 a7e2afc134f5e814\n\
biomes 2 0 0 1212 1f6d59d2c83fe2ba\n\
biomes 2 0 1 1212 f0fde992bebdceda\n\
biomes 2 0 2 1212 79f1a84934fef904\n\
biomes 2 0 3 1212 9353e6d86ba58ab6\n\
biomes 2 0 4 1212 ad0a230538a8b00c\n\
biomes 2 1 0 1212 1f6d59d2c83fe2ba\n\
biomes 2 1 1 1212 f0fde992bebdceda\n\
biomes 2 1 2 1212 79f1a84934fef904\n\
biomes 2 1 3 1212 9353e6d86ba58ab6\n\
biomes 2 1 4 1212 ad0a230538a8b00c\n\
names birch_forest=1096 desert=1004 forest=950 grove=1128 jungle=1158 meadow=850 minecraft:beach=100 minecraft:cold_ocean=100 minecraft:deep_cold_ocean=1990 minecraft:deep_frozen_ocean=3870 minecraft:deep_lukewarm_ocean=5490 minecraft:deep_ocean=9300 minecraft:frozen_ocean=130 minecraft:frozen_river=250 minecraft:ocean=250 minecraft:river=2440 minecraft:snowy_beach=40 minecraft:warm_ocean=200 null=228 plains=1092 snowy_plains=1016 swamp=1268 taiga=1112 windswept_hills=1298\n";

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
    const SEEDS: [i32; 6] = [0, 42, -17, i32::MAX, i32::MIN, -8_763_214];

    /// The oracle's `pools(manager, k)`: per climate type `c`, a pool of two biomes (weights 1 and 2; empty for
    /// `k == 3` and `c % 4 == 1`, so those climates take plains; weights 0 and 0 for `k == 4` and `c % 4 == 2`, so
    /// `getValue` is null), over one shared `Arc` per biome. The chunk-biome oracle uses `k` 0 to 3.
    pub(crate) fn pools<T>(palette: &[Arc<T>], k: usize) -> BiomePools<T> {
        let mut pools = BiomePools::default();
        for climate in BiomeType::VALUES {
            let c = climate as usize;
            let pair = vec![(c + k) % 10, (c + 2 * k + 3) % 10];
            let (weights, index): (&[f32], Vec<usize>) = if k == 3 && c % 4 == 1 {
                (&[], Vec::new())
            } else if k == 4 && c % 4 == 2 {
                (&[0.0, 0.0], pair)
            } else {
                (&[1.0, 2.0], pair)
            };
            let values = index.iter().map(|&i| Arc::clone(&palette[i])).collect();
            pools.insert(climate, Arc::new(WeightMap::new(values, weights)));
        }
        pools
    }

    fn lines() -> Vec<String> {
        let palette: Vec<Arc<String>> = NAMES
            .iter()
            .map(|name| Arc::new(name.to_string()))
            .collect();
        let plains = Arc::new("plains".to_string());
        let replace = |replacement: BiomeOverride| Arc::new(replacement.key().unwrap().to_string());
        let configs: Vec<_> = (0..5).map(|k| pools(&palette, k)).collect();
        let mut counts = BTreeMap::new();
        let mut lines = Vec::new();
        for variant in 0..3 {
            for set in 0..2 {
                let mut folds = [(0, 0xcbf2_9ce4_8422_2325_u64); 5];
                for seed in SEEDS {
                    let generator = generator(variant, set);
                    for (k, pools) in configs.iter().enumerate() {
                        let sampler = CurrentSurfaceBiomeSampler::new(&generator, pools, &plains);
                        let (columns, hash) = &mut folds[k];
                        for (x, z) in points() {
                            // Java's null result (`k == 4`) prints as "null".
                            let biome = match sampler.sample_biome(seed, x, z, replace) {
                                Ok(biome) => biome,
                                Err(NO_POOL_BIOME) => Arc::new("null".to_string()),
                                Err(error) => panic!("{error}"),
                            };
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
    fn surface_biomes_match_terraforged_java() {
        let expected: Vec<&str> = EXPECTED.lines().collect();
        let actual = lines();
        assert_eq!(actual.len(), expected.len());
        for (actual, expected) in actual.iter().zip(expected) {
            assert_eq!(actual, expected);
        }
    }
}
