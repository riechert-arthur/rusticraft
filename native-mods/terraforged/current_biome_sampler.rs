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

use super::{
    terraforged_biome_sampler::ClimateSample, terraforged_current_climate::CurrentClimateNoise,
    terraforged_noise_generator::NoiseGenerator,
};

pub struct CurrentBiomeSampler<'a> {
    frequency: f32,
    climate: CurrentClimateNoise,
    noise: &'a NoiseGenerator,
}

impl<'a> CurrentBiomeSampler<'a> {
    pub fn new(noise: &'a NoiseGenerator) -> Self {
        let context = noise.continent().context();
        let mut seed = context.seed;
        Self {
            frequency: noise.levels().noise_levels.frequency,
            climate: CurrentClimateNoise::new(&mut seed, context.settings.climate),
            noise,
        }
    }

    pub fn get_sample(&self, seed: i32, x: i32, z: i32) -> ClimateSample {
        let (x, z) = self.coordinates(x, z);
        let mut sample = ClimateSample::default();
        self.noise
            .continent()
            .sample_continent(seed, x, z, &mut sample.noise);
        self.noise
            .continent()
            .sample_river(seed, x, z, &mut sample.noise);
        self.climate.sample(seed, x, z, &mut sample);
        sample
    }

    pub fn get_shape(&self, seed: i32, x: i32, z: i32) -> f32 {
        let mut sample = ClimateSample::default();
        self.sample(seed, x, z, &mut sample);
        sample.biome_edge_noise
    }

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
        terraforged::Module,
        terraforged_climate::ClimateSettings,
        terraforged_context::{GeneratorContext, GeneratorSettings},
        terraforged_continent_noise::ContinentNoise,
        terraforged_engine::TerrainRegistry,
        terraforged_terrain_blender::{TerrainBlender, TerrainNoise},
        terraforged_terrain_levels::TerrainLevels,
    };
    use super::*;
    use std::sync::Arc;

    fn generator() -> NoiseGenerator {
        let levels = TerrainLevels::new(false, 1.0, -64, 480, 128, 62, 22);
        let terrains = Arc::new(TerrainRegistry::new());
        let mut settings = GeneratorSettings::default();
        settings.climate = ClimateSettings::terra_defaults();
        let continent = ContinentNoise::new(
            levels,
            GeneratorContext::new(settings),
            Arc::clone(&terrains),
        );
        let land = TerrainBlender::new(
            45_763_218,
            800,
            0.8,
            0.4,
            vec![Arc::new(TerrainNoise::new(
                terrains.get_by_name("hills").unwrap(),
                1.0,
                Module::constant(0.25),
            ))],
        );
        NoiseGenerator::new(levels, continent, land)
    }

    #[test]
    fn bridge_preserves_java_coordinate_and_sampling_order() {
        let generator = generator();
        let sampler = CurrentBiomeSampler::new(&generator);
        let frequency = generator.levels().noise_levels.frequency;
        for (seed, x, z) in [(0, 0, 0), (17, 125, -43), (-918_273, -3017, 871)] {
            let px = x as f32 * frequency;
            let pz = z as f32 * frequency;
            let mut expected = ClimateSample::default();
            generator
                .continent()
                .sample_continent(seed, px, pz, &mut expected.noise);
            generator
                .continent()
                .sample_river(seed, px, pz, &mut expected.noise);
            let context = generator.continent().context();
            let mut climate_seed = context.seed;
            CurrentClimateNoise::new(&mut climate_seed, context.settings.climate).sample(
                seed,
                px,
                pz,
                &mut expected,
            );

            let actual = sampler.get_sample(seed, x, z);
            assert_sample(&actual, &expected);
            assert_eq!(
                sampler.get_shape(seed, x, z).to_bits(),
                expected.biome_edge_noise.to_bits()
            );

            let mut climate_only = ClimateSample::default();
            climate_only.noise.continent_noise = 0.375;
            sampler.sample(seed, x, z, &mut climate_only);
            assert_eq!(
                climate_only.noise.continent_noise.to_bits(),
                0.375_f32.to_bits()
            );
            assert_eq!(
                climate_only.temperature.to_bits(),
                expected.temperature.to_bits()
            );
        }
    }

    fn assert_sample(actual: &ClimateSample, expected: &ClimateSample) {
        assert_eq!(actual.biome_noise.to_bits(), expected.biome_noise.to_bits());
        assert_eq!(
            actual.biome_edge_noise.to_bits(),
            expected.biome_edge_noise.to_bits()
        );
        assert_eq!(actual.moisture.to_bits(), expected.moisture.to_bits());
        assert_eq!(actual.temperature.to_bits(), expected.temperature.to_bits());
        assert_eq!(actual.climate_type, expected.climate_type);
        assert_eq!(
            actual.noise.continent_noise.to_bits(),
            expected.noise.continent_noise.to_bits()
        );
        assert_eq!(
            actual.noise.base_noise.to_bits(),
            expected.noise.base_noise.to_bits()
        );
        assert_eq!(
            actual.noise.river_noise.to_bits(),
            expected.noise.river_noise.to_bits()
        );
        assert_eq!(
            actual.noise.terrain_type.name(),
            expected.noise.terrain_type.name()
        );
    }
}
