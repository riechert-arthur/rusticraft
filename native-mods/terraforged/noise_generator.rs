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
    terraforged::{Module, Source, SourceConfig, SourceKind},
    terraforged_continent_noise::ContinentNoise,
    terraforged_continent_points::{BEACH, COAST, SHALLOW_OCEAN},
    terraforged_engine::{ControlPoints, Terrain, TerrainRegistry},
    terraforged_terrain_blender::{Blender, TerrainBlender},
    terraforged_terrain_data::{NoiseData, NoiseSample},
    terraforged_terrain_generator::NoiseGenerator as NoiseGeneratorContract,
    terraforged_terrain_levels::{NoiseLevels, TerrainLevels},
};

const OCEAN_OFFSET: i32 = 8_763_214;
const HEIGHT_MULTIPLIER: f32 = 1.2;

pub struct NoiseGenerator {
    levels: TerrainLevels,
    ocean: Module,
    land: TerrainBlender,
    continent: ContinentNoise,
    control_points: ControlPoints,
    deep_ocean: Terrain,
}

impl NoiseGeneratorContract for NoiseGenerator {
    fn levels(&self) -> NoiseLevels {
        self.levels.noise_levels
    }

    fn terrain_levels(&self) -> TerrainLevels {
        self.levels
    }

    fn height_noise(&self, seed: i32, x: i32, z: i32) -> f32 {
        let mut sample = NoiseSample::default();
        let mut blender = self.blender_resource();
        self.sample(seed, x, z, &mut sample, &mut blender)
            .height_noise
    }

    fn generate(
        &self,
        seed: i32,
        chunk_x: i32,
        chunk_z: i32,
        consumer: &mut dyn FnMut(&NoiseData),
    ) {
        let mut noise_data = NoiseData::new();
        let mut blender = self.blender_resource();
        let mut sample = NoiseSample::default();
        let start_x = chunk_x.wrapping_shl(4);
        let start_z = chunk_z.wrapping_shl(4);
        for delta_z in NoiseData::MIN..NoiseData::MAX {
            for delta_x in NoiseData::MIN..NoiseData::MAX {
                let x = start_x.wrapping_add(delta_x);
                let z = start_z.wrapping_add(delta_z);
                self.sample(seed, x, z, &mut sample, &mut blender);
                noise_data.set_noise(delta_x, delta_z, &sample);
            }
        }
        consumer(&noise_data);
    }
}

impl NoiseGenerator {
    pub fn new(levels: TerrainLevels, continent: ContinentNoise, land: TerrainBlender) -> Self {
        let control_points = continent.control_points();
        Self {
            levels,
            ocean: create_ocean_terrain(),
            land,
            continent,
            control_points,
            deep_ocean: TerrainRegistry::new().get_by_id(1),
        }
    }

    pub fn levels(&self) -> TerrainLevels {
        self.levels
    }

    pub fn continent(&self) -> &ContinentNoise {
        &self.continent
    }

    pub fn control_points(&self) -> ControlPoints {
        self.control_points
    }

    pub fn blender_resource(&self) -> Blender {
        self.land.blender_resource()
    }

    pub fn sample<'a>(
        &self,
        seed: i32,
        x: i32,
        z: i32,
        sample: &'a mut NoiseSample,
        blender: &mut Blender,
    ) -> &'a mut NoiseSample {
        let frequency = self.levels.noise_levels.frequency;
        let noise_x = x as f32 * frequency;
        let noise_z = z as f32 * frequency;
        self.sample_terrain(seed, noise_x, noise_z, sample, blender);
        self.sample_river(seed, noise_x, noise_z, sample)
    }

    pub fn sample_terrain<'a>(
        &self,
        seed: i32,
        x: f32,
        z: f32,
        sample: &'a mut NoiseSample,
        blender: &mut Blender,
    ) -> &'a mut NoiseSample {
        self.continent.sample_continent(seed, x, z, sample);
        self.apply_height(seed, x, z, sample, blender);
        sample
    }

    pub fn sample_river<'a>(
        &self,
        seed: i32,
        x: f32,
        z: f32,
        sample: &'a mut NoiseSample,
    ) -> &'a mut NoiseSample {
        self.continent.sample_river(seed, x, z, sample);
        sample
    }

    fn apply_height(
        &self,
        seed: i32,
        x: f32,
        z: f32,
        sample: &mut NoiseSample,
        blender: &mut Blender,
    ) {
        if sample.continent_noise < SHALLOW_OCEAN {
            let raw_noise = self
                .ocean
                .try_value_seed_first(seed, x, z)
                .expect("TerraForged ocean noise must support runtime seeding");
            sample.height_noise = self.levels.noise_levels.to_depth_noise(raw_noise);
            sample.terrain_type = self.deep_ocean.clone();
        } else if sample.continent_noise < BEACH {
            let lower_raw = self
                .ocean
                .try_value_seed_first(seed, x, z)
                .expect("TerraForged ocean noise must support runtime seeding");
            let lower = self.levels.noise_levels.to_depth_noise(lower_raw);
            let upper = self.levels.noise_levels.height_min;
            let alpha = (sample.continent_noise - SHALLOW_OCEAN) / (BEACH - SHALLOW_OCEAN);
            sample.height_noise = lower + alpha * (upper - lower);
        } else {
            let base_noise = sample.base_noise;
            let upper_raw = self.land.value_with(seed, x, z, blender) * HEIGHT_MULTIPLIER;
            let upper = self
                .levels
                .noise_levels
                .to_height_noise(base_noise, upper_raw);
            if sample.continent_noise < COAST {
                let lower = self.levels.noise_levels.height_min;
                let alpha = (sample.continent_noise - BEACH) / (COAST - BEACH);
                sample.height_noise = lower + alpha * (upper - lower);
            } else {
                sample.height_noise = upper;
            }
            sample.terrain_type = self.land.terrain(blender).clone();
        }
    }
}

fn create_ocean_terrain() -> Module {
    Module::source(Source::new(
        SourceKind::Simplex,
        SourceConfig {
            seed: OCEAN_OFFSET,
            octaves: 3,
            frequency: 1.0 / 64.0,
            ..SourceConfig::default()
        },
    ))
    .scale(Module::constant(0.4))
}

#[cfg(test)]
mod tests {
    use super::super::{
        terraforged_context::{GeneratorContext, GeneratorSettings},
        terraforged_continent_points::terrain_type,
        terraforged_engine::TerrainRegistry,
        terraforged_terrain_blender::TerrainNoise,
    };
    use super::*;
    use std::sync::Arc;

    fn generator() -> NoiseGenerator {
        let levels = TerrainLevels::new(false, 1.0, -64, 480, 128, 62, 22);
        let terrains = Arc::new(TerrainRegistry::new());
        let continent = ContinentNoise::new(
            levels,
            GeneratorContext::new(GeneratorSettings::default()),
            Arc::clone(&terrains),
        );
        let terrain = terrains.get_by_name("hills").unwrap();
        let land = TerrainBlender::new(
            45_763_218,
            800,
            0.8,
            0.4,
            vec![Arc::new(TerrainNoise::new(
                terrain,
                1.0,
                Module::constant(0.25),
            ))],
        );
        NoiseGenerator::new(levels, continent, land)
    }

    #[test]
    fn height_branches_match_current_java_generator() {
        let generator = generator();
        let mut blender = generator.blender_resource();
        let mut sample = NoiseSample::default();
        let cases = [
            (0x3e00_0000, 0x3d80_0000, 0x3d49_ef59, "deep_ocean"),
            (0x3ec0_0000, 0x3d80_0000, 0x3db6_c01a, "ocean"),
            (0x3f06_6666, 0x3e00_0000, 0x3e72_221a, "hills"),
            (0x3f20_0000, 0x3e00_0000, 0x3eb0_0000, "hills"),
        ];
        for (continent_bits, base_bits, expected_height, expected_terrain) in cases {
            sample.continent_noise = f32::from_bits(continent_bits);
            sample.base_noise = f32::from_bits(base_bits);
            sample.terrain_type = terrain_type(sample.continent_noise, &TerrainRegistry::new());
            generator.apply_height(42, 128.25, -64.5, &mut sample, &mut blender);
            assert_eq!(sample.height_noise.to_bits(), expected_height);
            assert_eq!(sample.terrain_type.name(), expected_terrain);
        }
    }

    #[test]
    fn bordered_chunk_generation_matches_direct_samples() {
        let seed = -17;
        let chunk_x: i32 = 3;
        let chunk_z: i32 = -5;
        let expected_generator = generator();
        let mut expected = Vec::new();
        let mut blender = expected_generator.blender_resource();
        let mut sample = NoiseSample::default();
        for delta_z in NoiseData::MIN..NoiseData::MAX {
            for delta_x in NoiseData::MIN..NoiseData::MAX {
                expected_generator.sample(
                    seed,
                    chunk_x.wrapping_shl(4).wrapping_add(delta_x),
                    chunk_z.wrapping_shl(4).wrapping_add(delta_z),
                    &mut sample,
                    &mut blender,
                );
                if [(-1, -1), (0, 0), (16, 16)].contains(&(delta_x, delta_z)) {
                    expected.push((delta_x, delta_z, sample.clone()));
                }
            }
        }
        let actual_generator = generator();
        let mut calls = 0;
        NoiseGeneratorContract::generate(&actual_generator, seed, chunk_x, chunk_z, &mut |data| {
            calls += 1;
            for (x, z, sample) in &expected {
                assert_eq!(
                    data.height().get(*x, *z).to_bits(),
                    sample.height_noise.to_bits()
                );
                assert_eq!(
                    data.base().get(*x, *z).to_bits(),
                    sample.base_noise.to_bits()
                );
                assert_eq!(
                    data.river().get(*x, *z).to_bits(),
                    sample.river_noise.to_bits()
                );
                assert_eq!(
                    data.terrain().get(*x, *z).unwrap().name(),
                    sample.terrain_type.name()
                );
            }
        });
        assert_eq!(calls, 1);

        let direct_generator = generator();
        let mut direct = NoiseSample::default();
        direct_generator.sample(
            seed,
            48,
            -80,
            &mut direct,
            &mut direct_generator.blender_resource(),
        );
        assert_eq!(
            NoiseGeneratorContract::height_noise(&generator(), seed, 48, -80).to_bits(),
            direct.height_noise.to_bits()
        );
    }
}
