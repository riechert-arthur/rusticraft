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

// Ported from TerraForged 6dd607eb
// (https://github.com/TerraForged/TerraForged/tree/6dd607ebfd41a3b7274e090b1f2cf565d53d1d44):
// `src/main/java/com/terraforged/mod/worldgen/biome/IBiomeSampler.java` (`Sampler` and `createClimate`), joining the
// continent noise to its `ClimateNoise`. MIT License, Copyright (c) 2021 TerraForged (above).

use super::{
    terraforged_biome_sampler::ClimateSample,
    terraforged_current_climate::{BiomeCell, CurrentClimateNoise},
    terraforged_noise_generator::NoiseGenerator,
};

/// `IBiomeSampler.Sampler`: the climate sample of a block column. With Horizon's `rain_shadow` on the generator
/// ([`NoiseGenerator::rain_shadow`]), each biome cell's moisture is TerraForged's times the cell's rain-shadow factor.
/// Block coordinates are scaled by the noise levels'
/// frequency (`x * levels.frequency`, an int-to-float conversion then a float product); [`Self::get_sample`] fills a
/// fresh sample with the continent ([`ContinentNoise::sample_continent`]), then the rivers
/// ([`ContinentNoise::sample_river`]), then the climate ([`CurrentClimateNoise::sample`]), all at the runtime seed.
/// Java hands out one reset thread-local sample; a new one per call is the same, since `reset` resets the
/// `NoiseSample` part and the climate writes every climate field.
///
/// The climate is `createClimate`'s: `new ClimateNoise(generator.getContinent().getContext())`, built from the
/// continent's context, so its seeds are drawn after the continent's four (`seed0`, `seed1`, two warp seeds). Java
/// draws them from the context it shares with the continent; [`ContinentNoise::context`] is read-only here, so the
/// climate draws from a copy of the context's seed in the same state, which gives the same seeds. The difference:
/// a second sampler built from one generator gets the same climate here, where Java's would draw the next seeds
/// (TerraForged builds one sampler per world, in `Source`).
///
/// Seed draws: both are 6dd607eb's, with Engine 0.3.0's post-increment `Seed`: root seed `s` gives the continent
/// `s..s+3` ([`ContinentNoise`]) and the climate its module seed `s + 4` and biome-warp seeds `s + 5`, `s + 6`
/// ([`CurrentClimateNoise::new`]; its moisture and temperature seeds start at the root plus their offsets). The
/// climate settings are the context's (`ClimateSettings::terraforged` for `createContinentNoise`).
///
/// [`ContinentNoise`]: super::terraforged_continent_noise::ContinentNoise
/// [`ContinentNoise::context`]: super::terraforged_continent_noise::ContinentNoise::context
/// [`ContinentNoise::sample_continent`]: super::terraforged_continent_noise::ContinentNoise::sample_continent
/// [`ContinentNoise::sample_river`]: super::terraforged_continent_noise::ContinentNoise::sample_river
pub struct CurrentBiomeSampler<'a> {
    frequency: f32,
    climate: CurrentClimateNoise,
    noise: &'a NoiseGenerator,
}

// TerraForged samples one sampler from its worker threads (`Source`'s `BiomeSampler`).
const _: fn() = || {
    fn shareable<T: Send + Sync>() {}
    shareable::<CurrentBiomeSampler<'static>>();
};

impl<'a> CurrentBiomeSampler<'a> {
    /// `new Sampler(noiseGenerator)`: the levels' frequency and `createClimate(noiseGenerator)` (see
    /// [`CurrentBiomeSampler`] for the seed).
    pub fn new(noise: &'a NoiseGenerator) -> Self {
        let context = noise.continent().context();
        let mut seed = context.seed;
        Self {
            frequency: noise.levels().noise_levels.frequency,
            climate: CurrentClimateNoise::new(&mut seed, context.settings.climate),
            noise,
        }
    }

    /// `getSample(seed, x, z)`: continent, rivers, then climate, into a fresh sample.
    pub fn get_sample(&self, seed: i32, x: i32, z: i32) -> ClimateSample {
        let (x, z) = self.coordinates(x, z);
        let continent = self.noise.continent();
        let mut sample = ClimateSample::new(&continent.context().terrain);
        continent.sample_continent(seed, x, z, &mut sample.noise);
        continent.sample_river(seed, x, z, &mut sample.noise);
        self.climate(seed, x, z, &mut sample);
        sample
    }

    /// `getShape(seed, x, z)`: the climate alone, into a fresh sample; its biome edge noise.
    pub fn get_shape(&self, seed: i32, x: i32, z: i32) -> f32 {
        let mut sample = ClimateSample::new(&self.noise.continent().context().terrain);
        self.sample(seed, x, z, &mut sample);
        sample.biome_edge_noise
    }

    /// `sample(seed, x, z, sample)`: the climate alone, into the caller's sample (its terrain fields stay).
    pub fn sample(&self, seed: i32, x: i32, z: i32, sample: &mut ClimateSample) {
        let (x, z) = self.coordinates(x, z);
        self.climate(seed, x, z, sample);
    }

    /// The biome cell block (`x`, `z`) takes its climate from.
    pub fn cell(&self, seed: i32, x: i32, z: i32) -> BiomeCell {
        let (x, z) = self.coordinates(x, z);
        let mut sample = ClimateSample::new(&self.noise.continent().context().terrain);
        let mut found = None;
        self.climate
            .sample_with(seed, x, z, &mut sample, |cell, moisture| {
                found = Some(cell);
                moisture
            });
        found.expect("the climate always reports its cell")
    }

    /// The climate at noise coordinates: TerraForged's, its moisture under Horizon's `rain_shadow` if the generator
    /// has it (one factor per biome cell, from the un-eroded terrain upwind; no drainage tiles).
    fn climate(&self, seed: i32, x: f32, z: f32, sample: &mut ClimateSample) {
        match self.noise.rain_shadow() {
            None => self.climate.sample(seed, x, z, sample),
            Some(shadow) => self
                .climate
                .sample_with(seed, x, z, sample, |cell, moisture| {
                    shadow.moisture(self.noise, seed, cell, moisture)
                }),
        }
    }

    fn coordinates(&self, x: i32, z: i32) -> (f32, f32) {
        (x as f32 * self.frequency, z as f32 * self.frequency)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::super::{
        terraforged_context::GeneratorContext,
        terraforged_continent_noise::ContinentNoise,
        terraforged_noise_generator::tests::{Fold, continent_settings, levels, terrain_set},
        terraforged_terrain_blender::TerrainBlender,
    };
    use super::*;

    /// `.local/terraforged/oracles/current_biome_sampler/` (`run.sh`, after `../noise_generator/run.sh`): 6dd607eb's
    /// real `IBiomeSampler.Sampler` and `ClimateNoise` (with Engine's seed-first `Moisture`/`Temperature`, from
    /// `../climate_noise`) over the newer source's real `NoiseGenerator` (as `noise_generator.rs`' oracle, a fresh
    /// generator per runtime seed). `continent` lines: a fold of `getSample`'s terrain fields per levels variant,
    /// terrain set and runtime seed. `climate` lines (terrain set 0): the context seed's next value after the
    /// sampler's constructor, then folds of `getSample`'s climate fields, of `getShape`, and of `sample()` into a
    /// sample whose terrain fields must stay. `freq` lines: the levels' frequency bits.
    const EXPECTED: &str = "\
freq 0 3f800000\n\
freq 1 3f800000\n\
freq 2 3f94df76\n\
continent 0 0 0 c590c5cf8c49462c\n\
continent 0 0 42 70f467d1f2e0c843\n\
continent 0 0 -17 feddf2dd35b38cd5\n\
continent 0 0 2147483647 c92b3c00564fea53\n\
continent 0 0 -2147483648 48b0feabf5d265e6\n\
continent 0 0 -8763214 484b2c3ed27fd6a2\n\
continent 0 1 0 c590c5cf8c49462c\n\
continent 0 1 42 70f467d1f2e0c843\n\
continent 0 1 -17 feddf2dd35b38cd5\n\
continent 0 1 2147483647 c92b3c00564fea53\n\
continent 0 1 -2147483648 48b0feabf5d265e6\n\
continent 0 1 -8763214 484b2c3ed27fd6a2\n\
continent 1 0 0 c590c5cf8c49462c\n\
continent 1 0 42 70f467d1f2e0c843\n\
continent 1 0 -17 feddf2dd35b38cd5\n\
continent 1 0 2147483647 c92b3c00564fea53\n\
continent 1 0 -2147483648 48b0feabf5d265e6\n\
continent 1 0 -8763214 484b2c3ed27fd6a2\n\
continent 1 1 0 c590c5cf8c49462c\n\
continent 1 1 42 70f467d1f2e0c843\n\
continent 1 1 -17 feddf2dd35b38cd5\n\
continent 1 1 2147483647 c92b3c00564fea53\n\
continent 1 1 -2147483648 48b0feabf5d265e6\n\
continent 1 1 -8763214 484b2c3ed27fd6a2\n\
continent 2 0 0 a7d45136e69159cd\n\
continent 2 0 42 75c6a6f08908552b\n\
continent 2 0 -17 12142f80c30fa5f8\n\
continent 2 0 2147483647 89e8fdb83d41cc3b\n\
continent 2 0 -2147483648 50bf3fe69b0e6fca\n\
continent 2 0 -8763214 ad5187f7d6fd8642\n\
continent 2 1 0 a7d45136e69159cd\n\
continent 2 1 42 75c6a6f08908552b\n\
continent 2 1 -17 12142f80c30fa5f8\n\
continent 2 1 2147483647 89e8fdb83d41cc3b\n\
continent 2 1 -2147483648 50bf3fe69b0e6fca\n\
continent 2 1 -8763214 ad5187f7d6fd8642\n\
climate 0 0 18749567 3002699f2b22e19d 4e90b49c4fcbfd2c ba7facada84ffb1f\n\
climate 0 42 18749567 a2825c48835579ce ca2ff7a30963dcc 7474db02fa85121e\n\
climate 0 -17 18749567 a2181bccf15e3dd6 bfbecd81d093a9c9 e436894a4a5c60c2\n\
climate 0 2147483647 18749567 61434c64b8040663 e21f9c9a791e8c62 7b06234da7aeac5f\n\
climate 0 -2147483648 18749567 62a626a616eaedca 630083cf2b93a5cc 437314b6899d2c00\n\
climate 0 -8763214 18749567 325f30209d1c77d ad5daec1320fd34a 781fa5fdd8b4bb11\n\
climate 1 0 18749567 3002699f2b22e19d 4e90b49c4fcbfd2c ba7facada84ffb1f\n\
climate 1 42 18749567 a2825c48835579ce ca2ff7a30963dcc 7474db02fa85121e\n\
climate 1 -17 18749567 a2181bccf15e3dd6 bfbecd81d093a9c9 e436894a4a5c60c2\n\
climate 1 2147483647 18749567 61434c64b8040663 e21f9c9a791e8c62 7b06234da7aeac5f\n\
climate 1 -2147483648 18749567 62a626a616eaedca 630083cf2b93a5cc 437314b6899d2c00\n\
climate 1 -8763214 18749567 325f30209d1c77d ad5daec1320fd34a 781fa5fdd8b4bb11\n\
climate 2 0 18749567 3c204645d7a1fdf9 a44d0ec126008120 71d303ad7526a3fb\n\
climate 2 42 18749567 9c0c927154333889 9d450e5a756a0cb5 5f3cbc89597caecf\n\
climate 2 -17 18749567 a7e23e01e2de9c1b b028c04728cc2e31 6e916a1f34c62d19\n\
climate 2 2147483647 18749567 91c8101c4cc0e09f 885050d295a27282 f7e6da11e710f133\n\
climate 2 -2147483648 18749567 5c6d696394040d47 a548c505a84f4ba6 1eb6642dfd40bf4d\n\
climate 2 -8763214 18749567 ff4abb376ab2b025 3534e2b60cd18d16 ce4ea38df48b9821\n\
";

    const SEEDS: [i32; 6] = [0, 42, -17, i32::MAX, i32::MIN, -8_763_214];
    const EDGE: [i32; 9] = [
        0,
        1,
        -1,
        i32::MAX,
        i32::MIN,
        16_777_217,
        -16_777_217,
        30_000_000,
        -29_999_999,
    ];

    /// `Points.points()`: every pair of edge coordinates, then an 11x11 grid.
    pub(crate) fn points() -> Vec<(i32, i32)> {
        let mut out = Vec::new();
        for x in EDGE {
            for z in EDGE {
                out.push((x, z));
            }
        }
        for i in -5..=5 {
            for j in -5..=5 {
                out.push((i * 1733 + 7, j * 2311 - 3));
            }
        }
        out
    }

    /// `noise_generator.rs`' oracle generator: `createContinentNoise`'s settings, its climate included.
    pub(crate) fn generator(variant: usize, set: usize) -> NoiseGenerator {
        let levels = levels(variant);
        let context = GeneratorContext::new(continent_settings(levels));
        let land = TerrainBlender::new(
            super::super::terraforged_noise_generator::TERRAIN_OFFSET,
            800,
            0.8,
            0.4,
            terrain_set(&context, set),
        );
        NoiseGenerator::new(levels, ContinentNoise::new(levels, context), land)
    }

    fn climate(fold: &mut Fold, sample: &ClimateSample) {
        fold.float(sample.biome_noise);
        fold.float(sample.biome_edge_noise);
        fold.float(sample.moisture);
        fold.float(sample.temperature);
        fold.add(sample.climate_type as u64);
    }

    fn lines() -> Vec<String> {
        let mut lines = Vec::new();
        for variant in 0..3 {
            let bits = levels(variant).noise_levels.frequency.to_bits();
            lines.push(format!("freq {variant} {bits:x}"));
        }
        for variant in 0..3 {
            for set in 0..2 {
                for seed in SEEDS {
                    let generator = generator(variant, set);
                    let sampler = CurrentBiomeSampler::new(&generator);
                    let mut fold = Fold::new();
                    for (x, z) in points() {
                        fold.sample(&sampler.get_sample(seed, x, z).noise);
                    }
                    lines.push(format!("continent {variant} {set} {seed} {:x}", fold.0));
                }
            }
        }
        for variant in 0..3 {
            for seed in SEEDS {
                let generator = generator(variant, 0);
                let sampler = CurrentBiomeSampler::new(&generator);
                let terrains = &generator.continent().context().terrain;
                let mut next = generator.continent().context().seed;
                // Climate: the module seed and the two warp seeds from the context's seed, then the next draw.
                for _ in 0..3 {
                    next.next();
                }
                let next = next.next();
                let mut folds = [Fold::new(), Fold::new(), Fold::new()];
                for (x, z) in points() {
                    climate(&mut folds[0], &sampler.get_sample(seed, x, z));
                    folds[1].float(sampler.get_shape(seed, x, z));
                    let mut sample = ClimateSample::new(terrains);
                    sample.noise.continent_noise = 0.375;
                    sample.noise.river_noise = -2.5;
                    sampler.sample(seed, x, z, &mut sample);
                    climate(&mut folds[2], &sample);
                    folds[2].float(sample.noise.continent_noise);
                    folds[2].float(sample.noise.river_noise);
                    assert!(sample.noise.terrain_type.is_same(&terrains.get_by_id(0)));
                    folds[2].add(i64::from(sample.noise.terrain_type.id()) as u64);
                }
                lines.push(format!(
                    "climate {variant} {seed} {next} {:x} {:x} {:x}",
                    folds[0].0, folds[1].0, folds[2].0
                ));
            }
        }
        lines
    }

    #[test]
    fn biome_sampler_matches_java_sampler() {
        let expected: Vec<&str> = EXPECTED.lines().collect();
        let actual = lines();
        assert_eq!(actual.len(), expected.len());
        for (actual, expected) in actual.iter().zip(expected) {
            assert_eq!(actual, expected);
        }
    }
}
