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
    terraforged::{floor, hash_2d},
    terraforged_continent_cell::{CellPoint, CellShape, CellSource},
    terraforged_continent_config::ContinentConfig,
    terraforged_continent_shape::ContinentCells,
};
use std::sync::Mutex;

pub struct CellGenerator {
    seed: i32,
    sample_seed: i32,
    jitter: f32,
    shape: CellShape,
    source: CellSource,
    threshold: f32,
    offset: Mutex<Option<(f32, f32)>>,
}

impl CellGenerator {
    pub fn new(config: ContinentConfig) -> Self {
        Self {
            seed: config.shape.seed0,
            sample_seed: config.shape.seed1.wrapping_add(6569),
            jitter: config.shape.jitter,
            shape: config.shape.cell_shape,
            source: config.shape.cell_source,
            threshold: config.shape.threshold,
            offset: Mutex::new(None),
        }
    }

    pub fn compute_cell(&self, seed: i32, cell_x: i32, cell_y: i32) -> CellPoint {
        let hash = hash_2d(self.seed.wrapping_add(seed), cell_x, cell_y);
        let px = self.shape.cell_x(hash, cell_x, cell_y, self.jitter);
        let py = self.shape.cell_y(hash, cell_x, cell_y, self.jitter);
        let seed = seed.wrapping_add(self.sample_seed);
        let x = px * 0.1;
        let y = py * 0.1;
        let noise0 = self.source.value(seed, x, y);
        let noise = (noise0 + self.source.value(seed, x * 2.75, y * 2.75) * 0.3) / 1.3;
        CellPoint {
            px,
            py,
            noise,
            noise0,
        }
    }

    pub fn world_offset(&self, seed: i32) -> (f32, f32) {
        let offset = *self
            .offset
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(offset) = offset {
            return offset;
        }
        let offset = self.compute_world_offset(seed, 100_000);
        *self
            .offset
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(offset);
        offset
    }

    fn compute_world_offset(&self, seed: i32, maximum_radius: i32) -> (f32, f32) {
        for radius in 1..=maximum_radius {
            for edge in 0..4 {
                for step in 0..radius * 2 {
                    let (cell_x, cell_y) = match edge {
                        0 => (-radius + step, -radius),
                        1 => (radius, -radius + step),
                        2 => (radius - step, radius),
                        _ => (-radius, radius - step),
                    };
                    let cell = self.compute_cell(seed, cell_x, cell_y);
                    if cell.noise < self.threshold {
                        continue;
                    }
                    if self.valid_spawn(seed, cell_x, cell_y) {
                        return (cell.px, cell.py);
                    }
                }
            }
        }
        (0.0, 0.0)
    }

    fn valid_spawn(&self, seed: i32, cell_x: i32, cell_y: i32) -> bool {
        for delta_y in -3..=3 {
            for delta_x in -3..=3 {
                let distance_squared = delta_x * delta_x + delta_y * delta_y;
                if delta_y < 1 || distance_squared >= 9 {
                    continue;
                }
                let cell = self.compute_cell(
                    seed,
                    cell_x.wrapping_add(delta_x),
                    cell_y.wrapping_add(delta_y),
                );
                if cell.noise < self.threshold {
                    return false;
                }
            }
        }
        true
    }
}

impl ContinentCells for CellGenerator {
    fn cell(&self, seed: i32, x: i32, y: i32) -> CellPoint {
        self.compute_cell(seed, x, y)
    }

    fn cell_shape(&self) -> CellShape {
        self.shape
    }

    fn nearest_cell(&self, seed: i32, x: f32, y: f32) -> (i32, i32) {
        let x = self.shape.adjust_x(x);
        let y = self.shape.adjust_y(y);
        let minimum_x = floor(x).wrapping_sub(1);
        let minimum_y = floor(y).wrapping_sub(1);
        let mut nearest = (0, 0);
        let mut distance = f32::MAX;
        for offset_y in 0..3 {
            for offset_x in 0..3 {
                let cell_x = minimum_x.wrapping_add(offset_x);
                let cell_y = minimum_y.wrapping_add(offset_y);
                let cell = self.cell(seed, cell_x, cell_y);
                let delta_x = x - cell.px;
                let delta_y = y - cell.py;
                let distance_squared = delta_x * delta_x + delta_y * delta_y;
                if distance_squared < distance {
                    distance = distance_squared;
                    nearest = (cell_x, cell_y);
                }
            }
        }
        nearest
    }
}

#[cfg(test)]
mod tests {
    use super::super::{
        terraforged_continent_shape::ShapeGenerator,
        terraforged_engine::{ControlPoints, WorldControlPoints},
        terraforged_terrain_data::NoiseSample,
    };
    use super::*;

    #[test]
    fn cell_and_shape_samples_match_current_java_source() {
        let expected = [
            [0xc0d4a223, 0x40691c38, 0x3ef5c142, 0x3ee9a0c4, 0, 0],
            [0xc0d4a223, 0x40691c38, 0x3e93d564, 0x3e88419e, 0, 0],
            [
                0xc0d4a223, 0x40691c38, 0x3f083765, 0x3f039950, 0x3f4eff8d, 0x3f800000,
            ],
            [0xc0ca5111, 0x40691c38, 0x3ef31e6a, 0x3ee49176, 0, 0],
            [0xc0ca5111, 0x40691c38, 0x3e973059, 0x3e5db16a, 0, 0],
            [
                0xc0ca5111, 0x40691c38, 0x3f0785ee, 0x3f03fefe, 0x3f4f82fb, 0x3f800000,
            ],
        ];
        for (shape_index, shape) in [CellShape::Square, CellShape::Hexagon]
            .into_iter()
            .enumerate()
        {
            for (source_index, source) in
                [CellSource::Perlin, CellSource::Simplex, CellSource::Cubic]
                    .into_iter()
                    .enumerate()
            {
                let mut config = ContinentConfig::default();
                config.shape.seed0 = i32::MAX;
                config.shape.seed1 = i32::MAX;
                config.shape.cell_shape = shape;
                config.shape.cell_source = source;
                let cells = CellGenerator::new(config);
                let cell = cells.cell(918273, -7, 3);
                assert_eq!(cells.nearest_cell(918273, -1.0, -0.75), (-2, -1));
                let generator =
                    ShapeGenerator::new(config, ControlPoints::new(WorldControlPoints::default()));
                let mut sample = NoiseSample::default();
                generator.sample(&cells, 918273, -1.0, -0.75, &mut sample);
                assert_eq!(
                    [
                        cell.px.to_bits(),
                        cell.py.to_bits(),
                        cell.noise.to_bits(),
                        cell.noise0.to_bits(),
                        sample.base_noise.to_bits(),
                        sample.continent_noise.to_bits(),
                    ],
                    expected[shape_index * 3 + source_index]
                );
            }
        }
    }

    #[test]
    fn spawn_offsets_match_current_java_source() {
        let expected = [
            (0xc015b35c, 0xc03c170b),
            (0xbff61748, 0xc01546e6),
            (0x3e757df8, 0xbfae835a),
            (0xc00ad9ae, 0xc03c170b),
            (0xbfbb0ba4, 0xc01546e6),
            (0x3e757df8, 0xbfae835a),
        ];
        for (shape_index, shape) in [CellShape::Square, CellShape::Hexagon]
            .into_iter()
            .enumerate()
        {
            for (source_index, source) in
                [CellSource::Perlin, CellSource::Simplex, CellSource::Cubic]
                    .into_iter()
                    .enumerate()
            {
                let mut config = ContinentConfig::default();
                config.shape.cell_shape = shape;
                config.shape.cell_source = source;
                let generator = CellGenerator::new(config);
                let offset = generator.world_offset(42);
                assert_eq!(
                    (offset.0.to_bits(), offset.1.to_bits()),
                    expected[shape_index * 3 + source_index]
                );
                assert_eq!(generator.world_offset(43), offset);
            }
        }
    }

    #[test]
    fn spawn_search_preserves_origin_skip_and_exhaustion() {
        let mut config = ContinentConfig::default();
        config.shape.threshold = f32::NAN;
        let generator = CellGenerator::new(config);
        let first = generator.compute_cell(42, -1, -1);
        assert_eq!(generator.compute_world_offset(42, 1), (first.px, first.py));
        assert_eq!(generator.compute_world_offset(42, 0), (0.0, 0.0));
        config.shape.threshold = f32::INFINITY;
        let generator = CellGenerator::new(config);
        assert_eq!(generator.compute_world_offset(42, 3), (0.0, 0.0));
    }

    #[test]
    fn samples_are_independent_of_call_order() {
        let generator = CellGenerator::new(ContinentConfig::default());
        let original = generator.cell(42, 3, -7);
        assert_ne!(original, generator.cell(43, 3, -7));
        generator.cell(42, 100, 200);
        assert_eq!(original, generator.cell(42, 3, -7));
    }
}
