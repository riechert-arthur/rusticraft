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
// `src/main/java/com/terraforged/mod/worldgen/biome/IBiomeSampler.java` (`Sampler` and `createClimate`; the file is
// the same in TerraForged 6dd607eb), joining the continent noise to Reimagined's `ClimateNoise`. MIT License,
// Copyright (c) 2020 TerraForged, Copyright (c) 2026 Romoslayer; see `LICENSE-TERRAFORGED-REIMAGINED`.

use super::{
    terraforged_biome_sampler::ClimateSample, terraforged_current_climate::CurrentClimateNoise,
    terraforged_noise_generator::NoiseGenerator,
};

/// `IBiomeSampler.Sampler`: the climate sample of a block column. Block coordinates are scaled by the noise levels'
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
/// Seed draws: the continent is 6dd607eb's ([`ContinentNoise`], Engine 0.3.0's post-increment `Seed`: root seed `s`
/// gives `s..s+3`) and the climate Reimagined's (pre-increment, [`CurrentClimateNoise::new`]). Both leave the seed
/// at `s + 4`, so the climate gets Reimagined's seeds (`s + 5` on); Reimagined's own continent, with its
/// pre-increment seed, draws `s+1..s+4` instead. The climate settings are the context's
/// (`ClimateSettings::terra_defaults` for Reimagined's `createContinentNoise`).
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
        self.climate.sample(seed, x, z, &mut sample);
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
        self.climate.sample(seed, x, z, sample);
    }

    fn coordinates(&self, x: i32, z: i32) -> (f32, f32) {
        (x as f32 * self.frequency, z as f32 * self.frequency)
    }
}

#[cfg(test)]
mod tests {
    use super::super::{
        terraforged_climate::ClimateSettings,
        terraforged_context::GeneratorContext,
        terraforged_continent_noise::ContinentNoise,
        terraforged_noise_generator::tests::{Fold, continent_settings, levels, terrain_set},
        terraforged_terrain_blender::TerrainBlender,
    };
    use super::*;

    /// `.local/terraforged/oracles/current_biome_sampler/` (`run.sh`, after `../noise_generator/run.sh`), in two
    /// halves since the two trees' Engine classes can't share a JVM. `continent` lines: 6dd607eb's real
    /// `IBiomeSampler.Sampler` over the newer source's real `NoiseGenerator` (as `noise_generator.rs`' oracle, a
    /// fresh generator per runtime seed), a fold of `getSample`'s terrain fields per levels variant, terrain set and
    /// runtime seed. `climate` lines: Reimagined's real `Sampler` and `ClimateNoise` with its own `Seed`, over
    /// `createContinentNoise`'s context and a stand-in continent that draws `ContinentNoise`'s four seeds: the
    /// context seed's next value after the constructor, then folds of `getSample`'s climate fields, of `getShape`,
    /// and of `sample()` into a sample whose terrain fields must stay. `freq` lines: the levels' frequency bits.
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
climate 0 0 18749568 883e1938b107e935 ae577161e1b56f2b 7ad25c5a1fa8bf15\n\
climate 0 42 18749568 542995659672f6b5 5949cef216b04673 6a535ad78244685\n\
climate 0 -17 18749568 64601abec611b773 59f4db3c7a4df791 3a13dcc9b113ac33\n\
climate 0 2147483647 18749568 91fa1f886b3d5fe1 8c0f841f29d2f77c eaba4931b54ce2d9\n\
climate 0 -2147483648 18749568 fcf6725856e3c613 97b3d98aff8e36bd 7ec5ecf2f5bc1d93\n\
climate 0 -8763214 18749568 5654d27be02530a 12126dbb03aad16c 5961c361b3c36732\n\
climate 1 0 18749568 883e1938b107e935 ae577161e1b56f2b 7ad25c5a1fa8bf15\n\
climate 1 42 18749568 542995659672f6b5 5949cef216b04673 6a535ad78244685\n\
climate 1 -17 18749568 64601abec611b773 59f4db3c7a4df791 3a13dcc9b113ac33\n\
climate 1 2147483647 18749568 91fa1f886b3d5fe1 8c0f841f29d2f77c eaba4931b54ce2d9\n\
climate 1 -2147483648 18749568 fcf6725856e3c613 97b3d98aff8e36bd 7ec5ecf2f5bc1d93\n\
climate 1 -8763214 18749568 5654d27be02530a 12126dbb03aad16c 5961c361b3c36732\n\
climate 2 0 18749568 45af1ed7abf5ebd4 cbdb7a23b59722e6 7a16ec4f1b25ce54\n\
climate 2 42 18749568 1315b99cc82fab39 ab4f9726130663e5 61dd94d2f9f61769\n\
climate 2 -17 18749568 81260bb5c67d0029 35ea48f40afe1dbe aa85719a71e6d3a9\n\
climate 2 2147483647 18749568 f056943313b24394 4ffe6d8fe9c419ee a27b08eb90bc6524\n\
climate 2 -2147483648 18749568 98972347c1aa0e14 5cf896953ba9f3b1 2b85bda68e6432bc\n\
climate 2 -8763214 18749568 25456bf27da37de6 889c0d15d53069bd 91b5f99798f9aba6\n\
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
    fn points() -> Vec<(i32, i32)> {
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

    /// `noise_generator.rs`' oracle generator, with Reimagined's `createContinentNoise` climate settings in the
    /// context (the continent doesn't read them).
    fn generator(variant: usize, set: usize) -> NoiseGenerator {
        let levels = levels(variant);
        let mut settings = continent_settings(levels);
        settings.climate = ClimateSettings::terra_defaults();
        let context = GeneratorContext::new(settings);
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
                // Climate: the module seed and the two warp seeds from the context's seed, then Java's `++seed`.
                for _ in 0..3 {
                    next.next();
                }
                let next = next.next().wrapping_add(1);
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
