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
    super::terrain::{TerrainBlockStates, TerrainChunk, TerrainError},
    terraforged_chunk_fill::fill_chunk,
    terraforged_eroded_noise_generator::ErodedNoiseGenerator,
    terraforged_terrain_cache::{TerrainCache, TerrainDataHandle, TerrainTask},
    terraforged_terrain_levels::TerrainLevels,
};
use crate::registry::BlockStateId;
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

    pub fn fill_from_noise<B: TerrainBlockStates>(
        &self,
        chunk: &mut TerrainChunk<'_, B>,
        stone: BlockStateId,
        water: BlockStateId,
    ) -> Result<TerrainDataHandle<ErodedNoiseGenerator>, TerrainError> {
        let position = chunk.position();
        let terrain = self.chunk_data(position.x, position.z);
        fill_chunk(self.sea_level(), chunk, &terrain, stone, water)?;
        Ok(terrain)
    }

    pub fn drop_chunk(&self, x: i32, z: i32) {
        self.terrain.remove(self.seed(), x, z);
    }

    pub fn base_height(&self, x: i32, z: i32, heightmap: TerraHeightmap) -> i32 {
        let seed = self.seed();
        let sample = self.terrain.noise_sample(seed, x, z);
        base_height_from_noise(
            self.levels,
            sample.base_noise,
            sample.height_noise,
            heightmap,
        )
    }

    pub fn base_column(&self, x: i32, z: i32) -> BaseColumn {
        let sample = self.terrain.noise_sample(self.seed(), x, z);
        base_column_from_noise(self.levels, sample.base_noise, sample.height_noise)
    }
}

fn base_height_from_noise(
    levels: TerrainLevels,
    base_noise: f32,
    height_noise: f32,
    heightmap: TerraHeightmap,
) -> i32 {
    let base = levels.height(levels.scaled_base_level(base_noise));
    let height = levels.height(levels.scaled_height(height_noise));
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
        terraforged::{Module, Source, SourceConfig, SourceKind},
        terraforged_context::{GeneratorContext, GeneratorSettings},
        terraforged_continent_noise::ContinentNoise,
        terraforged_engine::TerrainRegistry,
        terraforged_erosion::{ErosionFilterSettings, HeightmapSmoothingSettings},
        terraforged_noise_generator::NoiseGenerator,
        terraforged_terrain_blender::{TerrainBlender, TerrainNoise},
    };
    use super::*;
    use crate::{DATA_VERSION, heightmap::HeightmapState, nbt::Tag, registry::BlockRegistry};
    use horizon_core::position::ChunkPos;
    use std::sync::Arc;

    #[derive(Clone, Copy)]
    struct State(u32);

    impl HeightmapState for State {
        fn is_air(self) -> bool {
            self.0 < 2
        }

        fn is_air_block(self) -> bool {
            self.0 == 0
        }

        fn has_fluid(self) -> bool {
            self.0 == 3
        }

        fn blocks_motion_in_heightmap(self) -> bool {
            self.0 == 2
        }

        fn blocks_motion_in_heightmap_no_leaves(self) -> bool {
            self.0 == 2
        }
    }

    struct Properties;

    impl TerrainBlockStates for Properties {
        type State = State;

        fn heightmap_state(&self, id: BlockStateId) -> Self::State {
            State(id.0)
        }
    }

    fn registry() -> BlockRegistry {
        BlockRegistry::from_nbt(&Tag::compound([
            ("DataVersion", Tag::Int(DATA_VERSION)),
            (
                "states",
                Tag::List(vec![
                    Tag::compound([("id", Tag::String("minecraft:air".into()))]),
                    Tag::compound([("id", Tag::String("minecraft:void_air".into()))]),
                    Tag::compound([("id", Tag::String("minecraft:stone".into()))]),
                    Tag::compound([("id", Tag::String("minecraft:water".into()))]),
                ]),
            ),
            (
                "defaults",
                Tag::compound([
                    ("minecraft:air", Tag::Int(0)),
                    ("minecraft:void_air", Tag::Int(1)),
                    ("minecraft:stone", Tag::Int(2)),
                    ("minecraft:water", Tag::Int(3)),
                ]),
            ),
        ]))
        .unwrap()
    }

    fn generator() -> TerraForgedGenerator {
        generator_with_land(
            Module::constant(0.25),
            HeightmapSmoothingSettings::default(),
        )
    }

    fn generator_with_land(
        land_noise: Module,
        smoothing: HeightmapSmoothingSettings,
    ) -> TerraForgedGenerator {
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
                land_noise,
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
                smoothing,
            ),
        )
    }

    #[test]
    fn base_height_uses_raw_sample_even_when_smoothing_changes_terrain() {
        let generator = generator_with_land(
            Module::source(Source::new(
                SourceKind::Simplex,
                SourceConfig {
                    frequency: 0.2,
                    ..SourceConfig::default()
                },
            )),
            HeightmapSmoothingSettings {
                iterations: 2,
                smoothing_rate: 1.0,
                ..HeightmapSmoothingSettings::default()
            },
        );
        generator.capture_seed(7);
        let mut changed = false;
        for (x, z) in [(0, 0), (7, 11), (15, 15), (31, 17)] {
            let raw = generator.terrain.noise_sample(7, x, z);
            let eroded = generator.terrain.eroded_height_noise(7, x, z);
            let levels = generator.levels();
            changed |= levels.height(levels.scaled_height(raw.height_noise))
                != levels.height(levels.scaled_height(eroded));
            for heightmap in [
                TerraHeightmap::WorldSurface,
                TerraHeightmap::WorldSurfaceWorldgen,
                TerraHeightmap::MotionBlocking,
                TerraHeightmap::MotionBlockingNoLeaves,
                TerraHeightmap::OceanFloor,
                TerraHeightmap::OceanFloorWorldgen,
            ] {
                assert_eq!(
                    generator.base_height(x, z, heightmap),
                    base_height_from_noise(levels, raw.base_noise, raw.height_noise, heightmap)
                );
            }
        }
        assert!(
            changed,
            "fixture must distinguish raw from smoothed block heights"
        );
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

    #[test]
    fn fill_from_noise_uses_chunk_position_and_keeps_terrain_for_later_stages() {
        let generator = generator();
        generator.capture_seed(7);
        let registry = registry();
        let mut chunk =
            TerrainChunk::new(ChunkPos::new(0, 0), -64, 544, &registry, Properties).unwrap();
        let terrain = generator
            .fill_from_noise(&mut chunk, BlockStateId(2), BlockStateId(3))
            .unwrap();
        let cached = generator.chunk_data(0, 0);
        assert!(terrain.ptr_eq(&cached));

        let height = terrain.height_at(0, 0);
        let water = TerrainLevels::water_level(generator.sea_level(), 0, 0, &terrain);
        let top = height.max(water);
        assert_eq!(chunk.block(0, height, 0), BlockStateId(2));
        assert_eq!(
            chunk.block(0, top, 0),
            if top == height {
                BlockStateId(2)
            } else {
                BlockStateId(3)
            }
        );
        assert_eq!(chunk.block(0, top.wrapping_add(1), 0), BlockStateId(0));
    }
}
