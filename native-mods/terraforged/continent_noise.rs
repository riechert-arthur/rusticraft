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
    terraforged::{Source, SourceConfig, SourceKind},
    terraforged_context::GeneratorContext,
    terraforged_continent_carver::RiverCarver,
    terraforged_continent_cells::CellGenerator,
    terraforged_continent_config::ContinentConfig,
    terraforged_continent_points::terrain_type,
    terraforged_continent_shape::ShapeGenerator,
    terraforged_engine::{ControlPoints, TerrainRegistry},
    terraforged_river_network::RiverNetwork,
    terraforged_terrain_data::NoiseSample,
    terraforged_terrain_levels::TerrainLevels,
};
use std::sync::Arc;

pub struct ContinentNoise {
    context: GeneratorContext,
    control_points: ControlPoints,
    cells: Arc<CellGenerator>,
    shape: ShapeGenerator,
    rivers: RiverNetwork<Arc<CellGenerator>>,
    carver: RiverCarver,
    warp_x: Source,
    warp_y: Source,
    frequency: f32,
    terrains: Arc<TerrainRegistry>,
}

impl ContinentNoise {
    pub fn new(
        levels: TerrainLevels,
        mut context: GeneratorContext,
        terrains: Arc<TerrainRegistry>,
    ) -> Self {
        let control_points = ControlPoints::new(context.settings.world.control_points);
        let mut config = ContinentConfig::default();
        config.shape.scale = context.settings.world.continent.continent_scale;
        config.shape.seed0 = context.seed.next();
        config.shape.seed1 = context.seed.next();
        let cells = Arc::new(CellGenerator::new(config));
        let shape = ShapeGenerator::new(config, control_points);
        let carver = RiverCarver::new(levels.noise_levels, config);
        let rivers = RiverNetwork::new(Arc::clone(&cells), &shape, config);
        let frequency = 1.0 / config.shape.scale as f32;
        let source_config = SourceConfig {
            octaves: 3,
            lacunarity: 2.2,
            frequency: 3.0,
            gain: 0.3,
            ..SourceConfig::default()
        };
        let warp_x = Source::new(
            SourceKind::Perlin2,
            SourceConfig {
                seed: context.seed.next(),
                ..source_config
            },
        );
        let warp_y = Source::new(
            SourceKind::Perlin2,
            SourceConfig {
                seed: context.seed.next(),
                ..source_config
            },
        );
        Self {
            context,
            control_points,
            cells,
            shape,
            rivers,
            carver,
            warp_x,
            warp_y,
            frequency,
            terrains,
        }
    }

    pub fn context(&self) -> &GeneratorContext {
        &self.context
    }

    pub fn control_points(&self) -> ControlPoints {
        self.control_points
    }

    pub fn sample_continent(&self, seed: i32, x: f32, y: f32, sample: &mut NoiseSample) {
        let (point_x, point_y) = self.coordinates(seed, x, y);
        self.shape
            .sample(&self.cells, seed, point_x, point_y, sample);
        sample.terrain_type = terrain_type(sample.continent_noise, &self.terrains);
    }

    pub fn sample_river(&self, seed: i32, x: f32, y: f32, sample: &mut NoiseSample) {
        let (point_x, point_y) = self.coordinates(seed, x, y);
        self.rivers
            .sample(seed, point_x, point_y, sample, &self.carver, &self.terrains);
    }

    fn coordinates(&self, seed: i32, x: f32, y: f32) -> (f32, f32) {
        let x = x * self.frequency;
        let y = y * self.frequency;
        let point_x = x + (-0.5 + self.warp_x.value_seed_first(seed, x, y)) * 0.2;
        let point_y = y + (-0.5 + self.warp_y.value_seed_first(seed, x, y)) * 0.2;
        let (offset_x, offset_y) = self.cells.world_offset(seed);
        (point_x + offset_x, point_y + offset_y)
    }
}

#[cfg(test)]
mod tests {
    use super::super::terraforged_context::GeneratorSettings;
    use super::*;

    fn settings(scale: i32) -> GeneratorSettings {
        let mut settings = GeneratorSettings::default();
        settings.world.seed = 18749560;
        settings.world.continent.continent_scale = scale;
        settings.world.control_points.deep_ocean = 0.05;
        settings.world.control_points.shallow_ocean = 0.3;
        settings.world.control_points.beach = 0.45;
        settings.world.control_points.coast = 0.75;
        settings.world.control_points.inland = 0.8;
        settings
    }

    fn sample_hash(mut hash: u64, sample: &NoiseSample) -> u64 {
        for value in [
            sample.continent_noise,
            sample.base_noise,
            sample.height_noise,
            sample.river_noise,
        ] {
            hash = (hash ^ value.to_bits() as u64).wrapping_mul(1099511628211);
        }
        (hash ^ sample.terrain_type.id() as u64).wrapping_mul(1099511628211)
    }

    #[test]
    fn world_coordinate_samples_match_original_java_wrapper() {
        let expected = [
            0x9f2c0737a2d5def4,
            0x4ef9dd53d020ea0f,
            0x5d2fa993324f4465,
            0x76b4084ba462e7a7,
        ];
        let levels = TerrainLevels::new(false, 1.0, -64, 256, 128, 62, 22);
        for (scale_index, scale) in [400, 3000].into_iter().enumerate() {
            for (seed_index, seed) in [42, -17].into_iter().enumerate() {
                let continent = ContinentNoise::new(
                    levels,
                    GeneratorContext::new(settings(scale)),
                    Arc::new(TerrainRegistry::default()),
                );
                assert_eq!(continent.context().seed.get(), 18749564);
                let mut hash = 0xcbf29ce484222325;
                for (x, y) in [
                    (-4096.5, 2048.25),
                    (0.0, 0.0),
                    (123.25, -987.75),
                    (16384.0, -8192.0),
                ] {
                    let mut sample = NoiseSample::default();
                    continent.sample_continent(seed, x, y, &mut sample);
                    hash = sample_hash(hash, &sample);
                    sample.height_noise =
                        levels.noise_levels.to_height_noise(sample.base_noise, 0.35);
                    continent.sample_river(seed, x, y, &mut sample);
                    hash = sample_hash(hash, &sample);
                }
                assert_eq!(hash, expected[scale_index * 2 + seed_index]);
            }
        }
    }

    #[test]
    fn owned_continent_can_sample_from_parallel_workers() {
        let continent = ContinentNoise::new(
            TerrainLevels::default(),
            GeneratorContext::new(settings(400)),
            Arc::new(TerrainRegistry::default()),
        );
        let mut reference = NoiseSample::default();
        continent.sample_continent(42, 125.0, -87.0, &mut reference);
        let expected = sample_hash(0, &reference);
        std::thread::scope(|scope| {
            for _ in 0..4 {
                let continent = &continent;
                scope.spawn(move || {
                    let mut sample = NoiseSample::default();
                    continent.sample_continent(42, 125.0, -87.0, &mut sample);
                    assert_eq!(sample_hash(0, &sample), expected);
                });
            }
        });
    }
}
