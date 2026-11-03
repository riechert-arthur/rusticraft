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
    terraforged_engine::Terrain,
    terraforged_erosion::{
        ErosionFilter, ErosionFilterSettings, ErosionRegionCache, ErosionRegionLayout,
        ErosionResource, HeightmapSmoothing, HeightmapSmoothingSettings,
    },
    terraforged_noise_generator::NoiseGenerator,
    terraforged_terrain_data::{NoiseData, NoiseSample},
    terraforged_terrain_generator::NoiseGenerator as NoiseGeneratorContract,
    terraforged_terrain_levels::{NoiseLevels, TerrainLevels},
};
use std::{
    collections::VecDeque,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{Arc, Mutex},
};

struct NoiseResource {
    chunk: NoiseData,
    chunk_samples: Vec<NoiseSample>,
}

impl Default for NoiseResource {
    fn default() -> Self {
        let length = NoiseData::MAX.wrapping_sub(NoiseData::MIN) as usize;
        Self {
            chunk: NoiseData::new(),
            chunk_samples: std::iter::repeat_with(NoiseSample::default)
                .take(length * length)
                .collect(),
        }
    }
}

pub struct ErodedNoiseGenerator {
    erosion: ErosionFilter,
    generator: NoiseGenerator,
    smoothing: HeightmapSmoothingSettings,
    resources: Mutex<VecDeque<NoiseResource>>,
    regions: ErosionRegionCache<Vec<f32>>,
    /// The `deterministic` setting: reset each sample's river noise (see `generate`).
    fresh_rivers: bool,
}

impl ErodedNoiseGenerator {
    pub fn new(
        generator: NoiseGenerator,
        erosion: ErosionFilterSettings,
        smoothing: HeightmapSmoothingSettings,
    ) -> Self {
        Self {
            erosion: ErosionFilter::new(ErosionRegionLayout::MAP_SIZE, erosion),
            generator,
            smoothing,
            resources: Mutex::new(VecDeque::new()),
            regions: ErosionRegionCache::new(),
            fresh_rivers: super::terraforged_settings::current().deterministic,
        }
    }

    pub fn generator(&self) -> &NoiseGenerator {
        &self.generator
    }

    pub fn noise_sample(&self, seed: i32, x: i32, z: i32) -> NoiseSample {
        let mut sample = NoiseSample::default();
        self.sample(seed, x, z, &mut sample);
        sample
    }

    pub fn sample<'a>(
        &self,
        seed: i32,
        x: i32,
        z: i32,
        sample: &'a mut NoiseSample,
    ) -> &'a mut NoiseSample {
        let mut blender = self.generator.blender_resource();
        self.generator.sample(seed, x, z, sample, &mut blender)
    }

    pub fn eroded_height_noise(&self, seed: i32, x: i32, z: i32) -> f32 {
        let mut sample = NoiseSample::default();
        let frequency = self.generator.levels().noise_levels.frequency;
        let noise_x = x as f32 * frequency;
        let noise_z = z as f32 * frequency;
        let mut blender = self.generator.blender_resource();
        self.generator
            .sample_terrain(seed, noise_x, noise_z, &mut sample, &mut blender);
        sample.height_noise = self.eroded_height(seed, x, z);
        self.generator
            .sample_river(seed, noise_x, noise_z, &mut sample)
            .height_noise
    }

    pub fn eroded_height(&self, seed: i32, x: i32, z: i32) -> f32 {
        let region = self.region(seed, x >> 4, z >> 4);
        region[ErosionRegionLayout::block_index(x, z)]
    }

    pub fn find(
        &self,
        seed: i32,
        x: i32,
        z: i32,
        min_radius: i32,
        max_radius: i32,
        terrain: &Terrain,
    ) -> i64 {
        self.generator
            .find(seed, x, z, min_radius, max_radius, terrain)
    }

    fn region(&self, seed: i32, chunk_x: i32, chunk_z: i32) -> Arc<Vec<f32>> {
        self.regions
            .get_or_build(seed, chunk_x, chunk_z, |region_x, region_z| {
                self.build_region(seed, region_x, region_z)
            })
    }

    fn build_region(&self, seed: i32, region_x: i32, region_z: i32) -> Vec<f32> {
        let (origin_chunk_x, origin_chunk_z) =
            ErosionRegionLayout::origin_chunk(region_x, region_z);
        let start_x = origin_chunk_x.wrapping_shl(4);
        let start_z = origin_chunk_z.wrapping_shl(4);
        let map_size = ErosionRegionLayout::MAP_SIZE;
        let mut map = vec![0.0; map_size.wrapping_mul(map_size) as usize];
        let mut sample = NoiseSample::default();
        let mut blender = self.generator.blender_resource();
        let frequency = self.generator.levels().noise_levels.frequency;

        for z in 0..map_size {
            let noise_z = start_z.wrapping_add(z) as f32 * frequency;
            let row = z.wrapping_mul(map_size);
            for x in 0..map_size {
                let noise_x = start_x.wrapping_add(x) as f32 * frequency;
                map[row.wrapping_add(x) as usize] = self
                    .generator
                    .sample_terrain(seed, noise_x, noise_z, &mut sample, &mut blender)
                    .height_noise;
            }
        }

        self.erosion.apply_region(
            seed,
            origin_chunk_x,
            origin_chunk_z,
            ErosionRegionLayout::TOTAL_CHUNKS,
            map_size,
            &mut ErosionResource::default(),
            &mut map,
        );
        HeightmapSmoothing::apply(&mut map, map_size, self.smoothing);
        map
    }

    fn take_resource(&self) -> NoiseResource {
        self.resources
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .pop_front()
            .unwrap_or_default()
    }

    fn restore_resource(&self, resource: NoiseResource) {
        self.resources
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push_back(resource);
    }
}

impl NoiseGeneratorContract for ErodedNoiseGenerator {
    fn levels(&self) -> NoiseLevels {
        self.generator.levels().noise_levels
    }

    fn terrain_levels(&self) -> TerrainLevels {
        self.generator.levels()
    }

    fn height_noise(&self, seed: i32, x: i32, z: i32) -> f32 {
        NoiseGeneratorContract::height_noise(&self.generator, seed, x, z)
    }

    fn generate(
        &self,
        seed: i32,
        chunk_x: i32,
        chunk_z: i32,
        consumer: &mut dyn FnMut(&NoiseData),
    ) {
        let region = self.region(seed, chunk_x, chunk_z);
        let offset_x = ErosionRegionLayout::chunk_offset(chunk_x);
        let offset_z = ErosionRegionLayout::chunk_offset(chunk_z);
        let mut resource = self.take_resource();
        let result = catch_unwind(AssertUnwindSafe(|| {
            let mut blender = self.generator.blender_resource();
            let start_x = chunk_x.wrapping_shl(4);
            let start_z = chunk_z.wrapping_shl(4);
            let frequency = self.generator.levels().noise_levels.frequency;
            for delta_z in NoiseData::MIN..NoiseData::MAX {
                let noise_z = start_z.wrapping_add(delta_z) as f32 * frequency;
                let row = offset_z
                    .wrapping_add(delta_z)
                    .wrapping_mul(ErosionRegionLayout::MAP_SIZE);
                for delta_x in NoiseData::MIN..NoiseData::MAX {
                    let noise_x = start_x.wrapping_add(delta_x) as f32 * frequency;
                    let index = resource.chunk.height().index().of(delta_x, delta_z);
                    let sample = &mut resource.chunk_samples[index];
                    self.generator
                        .sample_terrain(seed, noise_x, noise_z, sample, &mut blender);
                    sample.height_noise =
                        region[row.wrapping_add(offset_x).wrapping_add(delta_x) as usize];
                    // The original reuses its samples from chunk to chunk without resetting
                    // the river noise, which the carver only multiplies: a column keeps
                    // the product of every chunk its worker sampled before, so after a few
                    // chunks it is 0 inland (wet everywhere, the forests TerraForged worlds
                    // are known for) but not on a worker's first chunks. Deterministic
                    // worlds start at that settled 0; the carver still sets 1 seaward of
                    // the beach.
                    if self.fresh_rivers {
                        sample.river_noise = 0.0;
                    }
                    self.generator.sample_river(seed, noise_x, noise_z, sample);
                    resource.chunk.set_noise_index(index, sample);
                }
            }
            consumer(&resource.chunk);
        }));
        self.restore_resource(resource);
        if result.is_err() {
            eprintln!(
                "Eroded noise failed for chunk {chunk_x}, {chunk_z} -- that chunk will be left unfilled"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{
        terraforged_context::{GeneratorContext, GeneratorSettings},
        terraforged_continent_noise::ContinentNoise,
        terraforged_engine::TerrainRegistry,
        terraforged_terrain_blender::{TerrainBlender, TerrainNoise},
        terraforged_terrain_levels::TerrainLevels,
    };
    use super::*;

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
                super::super::terraforged::Module::constant(0.25),
            ))],
        );
        NoiseGenerator::new(levels, continent, land)
    }

    #[test]
    fn unfiltered_eroded_heights_match_raw_generation() {
        let eroded = ErodedNoiseGenerator::new(
            generator(),
            ErosionFilterSettings {
                droplets_per_chunk: 0,
                ..ErosionFilterSettings::default()
            },
            HeightmapSmoothingSettings::default(),
        );
        let seed = -17;
        for (x, z) in [(0, 0), (15, 15), (16, 31), (127, 127)] {
            let mut sample = NoiseSample::default();
            let mut blender = eroded.generator.blender_resource();
            let expected = eroded
                .generator
                .sample(seed, x, z, &mut sample, &mut blender)
                .height_noise;
            assert_eq!(
                eroded.eroded_height_noise(seed, x, z).to_bits(),
                expected.to_bits()
            );
        }
    }

    #[test]
    fn region_map_uses_margin_origin_and_java_row_order() {
        let eroded = ErodedNoiseGenerator::new(
            generator(),
            ErosionFilterSettings {
                droplets_per_chunk: 0,
                ..ErosionFilterSettings::default()
            },
            HeightmapSmoothingSettings::default(),
        );
        let seed = 42;
        let region = eroded.build_region(seed, -1, 2);
        assert_eq!(region.len(), 256 * 256);
        let (origin_chunk_x, origin_chunk_z) = ErosionRegionLayout::origin_chunk(-1, 2);
        let mut sample = NoiseSample::default();
        let mut blender = eroded.generator.blender_resource();
        let frequency = eroded.generator.levels().noise_levels.frequency;
        for (x, z) in [(0, 0), (255, 0), (0, 255), (255, 255), (91, 173)] {
            let world_x = origin_chunk_x.wrapping_shl(4).wrapping_add(x);
            let world_z = origin_chunk_z.wrapping_shl(4).wrapping_add(z);
            let expected = eroded
                .generator
                .sample_terrain(
                    seed,
                    world_x as f32 * frequency,
                    world_z as f32 * frequency,
                    &mut sample,
                    &mut blender,
                )
                .height_noise;
            assert_eq!(
                region[z
                    .wrapping_mul(ErosionRegionLayout::MAP_SIZE)
                    .wrapping_add(x) as usize]
                    .to_bits(),
                expected.to_bits()
            );
        }
    }

    #[test]
    fn bordered_chunk_generation_uses_eroded_height_and_raw_metadata() {
        let eroded = ErodedNoiseGenerator::new(
            generator(),
            ErosionFilterSettings {
                droplets_per_chunk: 1,
                droplet_lifetime: 4,
                ..ErosionFilterSettings::default()
            },
            HeightmapSmoothingSettings {
                iterations: 1,
                ..HeightmapSmoothingSettings::default()
            },
        );
        let seed = 91;
        let chunk_x: i32 = -3;
        let chunk_z: i32 = 5;
        let region = eroded.region(seed, chunk_x, chunk_z);
        let offset_x = ErosionRegionLayout::chunk_offset(chunk_x);
        let offset_z = ErosionRegionLayout::chunk_offset(chunk_z);
        let frequency = eroded.generator.levels().noise_levels.frequency;
        let mut blender = eroded.generator.blender_resource();
        let mut expected = NoiseSample::default();
        let mut expected_values = Vec::new();
        for delta_z in NoiseData::MIN..NoiseData::MAX {
            let world_z = chunk_z.wrapping_shl(4).wrapping_add(delta_z);
            let row = offset_z
                .wrapping_add(delta_z)
                .wrapping_mul(ErosionRegionLayout::MAP_SIZE);
            for delta_x in NoiseData::MIN..NoiseData::MAX {
                let world_x = chunk_x.wrapping_shl(4).wrapping_add(delta_x);
                eroded.generator.sample_terrain(
                    seed,
                    world_x as f32 * frequency,
                    world_z as f32 * frequency,
                    &mut expected,
                    &mut blender,
                );
                expected.height_noise =
                    region[row.wrapping_add(offset_x).wrapping_add(delta_x) as usize];
                eroded.generator.sample_river(
                    seed,
                    world_x as f32 * frequency,
                    world_z as f32 * frequency,
                    &mut expected,
                );
                expected_values.push((
                    expected.height_noise.to_bits(),
                    expected.base_noise.to_bits(),
                    expected.river_noise.to_bits(),
                    expected.terrain_type.name().to_owned(),
                ));
            }
        }
        let mut calls = 0;
        let mut actual_values = Vec::new();
        NoiseGeneratorContract::generate(&eroded, seed, chunk_x, chunk_z, &mut |chunk| {
            calls += 1;
            for delta_z in NoiseData::MIN..NoiseData::MAX {
                for delta_x in NoiseData::MIN..NoiseData::MAX {
                    actual_values.push((
                        chunk.height().get(delta_x, delta_z).to_bits(),
                        chunk.base().get(delta_x, delta_z).to_bits(),
                        chunk.river().get(delta_x, delta_z).to_bits(),
                        chunk
                            .terrain()
                            .get(delta_x, delta_z)
                            .unwrap()
                            .name()
                            .to_owned(),
                    ));
                }
            }
        });
        assert_eq!(calls, 1);
        assert_eq!(actual_values, expected_values);
    }

    #[test]
    fn contract_height_and_find_stay_on_raw_generator() {
        let eroded = ErodedNoiseGenerator::new(
            generator(),
            ErosionFilterSettings::default(),
            HeightmapSmoothingSettings::default(),
        );
        let seed = 31;
        assert_eq!(
            NoiseGeneratorContract::height_noise(&eroded, seed, 80, -32).to_bits(),
            NoiseGeneratorContract::height_noise(&eroded.generator, seed, 80, -32).to_bits()
        );
        let terrain = TerrainRegistry::new().get_by_name("hills").unwrap();
        assert_eq!(
            eroded.find(seed, 0, 0, 1, 4, &terrain),
            eroded.generator.find(seed, 0, 0, 1, 4, &terrain)
        );
    }

    #[test]
    fn consumer_failure_still_restores_scratch_resource() {
        let eroded = ErodedNoiseGenerator::new(
            generator(),
            ErosionFilterSettings {
                droplets_per_chunk: 0,
                ..ErosionFilterSettings::default()
            },
            HeightmapSmoothingSettings::default(),
        );
        NoiseGeneratorContract::generate(&eroded, 7, 0, 0, &mut |_| panic!("consumer failed"));
        assert_eq!(
            eroded
                .resources
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .len(),
            1
        );
    }
}
