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
    terraforged_eroded_noise_generator::ErodedNoiseGenerator,
    terraforged_terrain_cache::{TerrainCache, TerrainDataHandle, TerrainTask},
    terraforged_terrain_levels::TerrainLevels,
};
use std::sync::atomic::{AtomicI32, AtomicI64, Ordering};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TerraHeightmap {
    WorldSurface,
    WorldSurfaceWorldgen,
    MotionBlocking,
    MotionBlockingNoLeaves,
    OceanFloor,
    OceanFloorWorldgen,
}

impl TerraHeightmap {
    const fn includes_base(self) -> bool {
        matches!(
            self,
            Self::WorldSurface
                | Self::WorldSurfaceWorldgen
                | Self::MotionBlocking
                | Self::MotionBlockingNoLeaves
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ColumnBlock {
    Stone,
    Water,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BaseColumn {
    pub min_y: i32,
    pub states: Vec<ColumnBlock>,
}

pub struct TerraForgedGenerator {
    levels: TerrainLevels,
    terrain: TerrainCache<ErodedNoiseGenerator>,
    seed: AtomicI32,
    level_seed: AtomicI64,
}

impl TerraForgedGenerator {
    pub fn new(levels: TerrainLevels, noise: ErodedNoiseGenerator) -> Self {
        Self {
            levels,
            terrain: TerrainCache::new(levels, noise),
            seed: AtomicI32::new(0),
            level_seed: AtomicI64::new(0),
        }
    }

    pub fn capture_seed(&self, level_seed: i64) {
        self.seed.store(level_seed as i32, Ordering::Release);
        self.level_seed.store(level_seed, Ordering::Release);
    }

    pub fn seed(&self) -> i32 {
        self.seed.load(Ordering::Acquire)
    }

    pub fn level_seed(&self) -> i64 {
        self.level_seed.load(Ordering::Acquire)
    }

    pub const fn levels(&self) -> TerrainLevels {
        self.levels
    }

    pub const fn min_y(&self) -> i32 {
        self.levels.min_y
    }

    pub const fn sea_level(&self) -> i32 {
        self.levels.sea_level
    }

    pub const fn gen_depth(&self) -> i32 {
        self.levels.max_y
    }

    pub fn hint_chunk(&self, x: i32, z: i32) {
        self.terrain.hint(self.seed(), x, z);
    }

    pub fn chunk_data(&self, x: i32, z: i32) -> TerrainDataHandle<ErodedNoiseGenerator> {
        self.terrain.get_now(self.seed(), x, z)
    }

    pub fn chunk_data_async(&self, x: i32, z: i32) -> TerrainTask<ErodedNoiseGenerator> {
        self.terrain.get_async(self.seed(), x, z)
    }

    pub fn drop_chunk(&self, x: i32, z: i32) {
        self.terrain.remove(self.seed(), x, z);
    }

    pub fn base_height(&self, x: i32, z: i32, heightmap: TerraHeightmap) -> i32 {
        let seed = self.seed();
        let sample = self.terrain.noise_sample(seed, x, z);
        let eroded_height = self.terrain.eroded_height_noise(seed, x, z);
        base_height_from_noise(self.levels, sample.base_noise, eroded_height, heightmap)
    }

    pub fn base_column(&self, x: i32, z: i32) -> BaseColumn {
        let sample = self.terrain.noise_sample(self.seed(), x, z);
        base_column_from_noise(self.levels, sample.base_noise, sample.height_noise)
    }
}

fn base_height_from_noise(
    levels: TerrainLevels,
    base_noise: f32,
    eroded_height_noise: f32,
    heightmap: TerraHeightmap,
) -> i32 {
    let base = levels.height(levels.scaled_base_level(base_noise));
    let height = levels.height(levels.scaled_height(eroded_height_noise));
    if heightmap.includes_base() {
        base.max(height).wrapping_add(1)
    } else {
        height.wrapping_add(1)
    }
}

fn base_column_from_noise(levels: TerrainLevels, base_noise: f32, height_noise: f32) -> BaseColumn {
    let base = levels.height(levels.scaled_base_level(base_noise));
    let height = levels.height(levels.scaled_height(height_noise));
    let surface = base.max(height);
    let length = usize::try_from(surface).expect("TerraForged base-column surface is non-negative");
    let stone = usize::try_from(height).expect("TerraForged base-column height is non-negative");
    let mut states = vec![ColumnBlock::Stone; length];
    states[stone..].fill(ColumnBlock::Water);
    BaseColumn {
        min_y: height,
        states,
    }
}

#[cfg(test)]
mod tests {
    use super::super::{
        terraforged::Module,
        terraforged_context::{GeneratorContext, GeneratorSettings},
        terraforged_continent_noise::ContinentNoise,
        terraforged_engine::TerrainRegistry,
        terraforged_erosion::{ErosionFilterSettings, HeightmapSmoothingSettings},
        terraforged_noise_generator::NoiseGenerator,
        terraforged_terrain_blender::{TerrainBlender, TerrainNoise},
    };
    use super::*;
    use std::sync::Arc;

    fn generator() -> TerraForgedGenerator {
        let levels = TerrainLevels::new(false, 1.0, -64, 480, 128, 62, 22);
        let terrains = Arc::new(TerrainRegistry::new());
        let continent = ContinentNoise::new(
            levels,
            GeneratorContext::new(GeneratorSettings::default()),
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
        TerraForgedGenerator::new(
            levels,
            ErodedNoiseGenerator::new(
                NoiseGenerator::new(levels, continent, land),
                ErosionFilterSettings {
                    droplets_per_chunk: 0,
                    ..ErosionFilterSettings::default()
                },
                HeightmapSmoothingSettings::default(),
            ),
        )
    }

    #[test]
    fn query_math_matches_current_java_source() {
        let levels = TerrainLevels::new(false, 1.0, -64, 480, 128, 62, 22);
        for (base, height, surface, ocean) in [
            (0.25, 0.4, 193, 193),
            (0.25, 0.1, 95, 49),
            (-0.5, 0.2, 97, 97),
        ] {
            assert_eq!(
                base_height_from_noise(levels, base, height, TerraHeightmap::WorldSurface),
                surface
            );
            assert_eq!(
                base_height_from_noise(levels, base, height, TerraHeightmap::OceanFloor),
                ocean
            );
        }
        let low = base_column_from_noise(levels, 0.25, 0.1);
        assert_eq!((low.min_y, low.states.len()), (48, 94));
        assert_eq!(
            low.states
                .iter()
                .filter(|state| **state == ColumnBlock::Stone)
                .count(),
            48
        );
        assert_eq!(
            low.states
                .iter()
                .filter(|state| **state == ColumnBlock::Water)
                .count(),
            46
        );
        let high = base_column_from_noise(levels, 0.25, 0.4);
        assert_eq!((high.min_y, high.states.len()), (192, 192));
        assert!(high.states.iter().all(|state| *state == ColumnBlock::Stone));
    }

    #[test]
    fn facade_captures_seed_and_routes_cache_queries() {
        let generator = generator();
        let level_seed = 0x1234_5678_89ab_cdef_u64 as i64;
        generator.capture_seed(level_seed);
        assert_eq!(generator.seed(), 0x89ab_cdef_u32 as i32);
        assert_eq!(generator.level_seed(), level_seed);
        assert_eq!(
            (
                generator.min_y(),
                generator.sea_level(),
                generator.gen_depth()
            ),
            (-64, 62, 480)
        );
        let task = generator.chunk_data_async(-2, 3);
        generator.hint_chunk(-2, 3);
        let first = task.join();
        let second = generator.chunk_data(-2, 3);
        assert!(first.ptr_eq(&second));
        let surface = generator.base_height(8, -11, TerraHeightmap::WorldSurface);
        let ocean = generator.base_height(8, -11, TerraHeightmap::OceanFloor);
        assert!(surface >= ocean);
        generator.drop_chunk(-2, 3);
    }
}
