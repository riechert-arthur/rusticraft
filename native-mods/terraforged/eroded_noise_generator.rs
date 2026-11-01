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
    terraforged_erosion::{
        ErosionFilter, ErosionFilterSettings, ErosionRegionCache, ErosionRegionLayout,
        ErosionResource, HeightmapSmoothing, HeightmapSmoothingSettings,
    },
    terraforged_noise_generator::NoiseGenerator,
    terraforged_terrain_data::NoiseSample,
};
use std::sync::Arc;

pub struct ErodedNoiseGenerator {
    erosion: ErosionFilter,
    generator: NoiseGenerator,
    smoothing: HeightmapSmoothingSettings,
    regions: ErosionRegionCache<Vec<f32>>,
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
            regions: ErosionRegionCache::new(),
        }
    }

    pub fn generator(&self) -> &NoiseGenerator {
        &self.generator
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
}
