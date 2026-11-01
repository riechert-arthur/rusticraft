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
    terraforged_continent_cell::{CellPoint, CellShape},
    terraforged_continent_config::ContinentConfig,
    terraforged_continent_points::{FalloffPoint, falloff, falloff_curve},
    terraforged_engine::ControlPoints,
    terraforged_terrain_data::NoiseSample,
};

pub trait ContinentCells {
    fn nearest_cell(&self, seed: i32, x: f32, y: f32) -> (i32, i32);
    fn cell(&self, seed: i32, x: i32, y: i32) -> CellPoint;
    fn cell_shape(&self) -> CellShape;
}

impl<Cells: ContinentCells + ?Sized> ContinentCells for &Cells {
    fn nearest_cell(&self, seed: i32, x: f32, y: f32) -> (i32, i32) {
        (**self).nearest_cell(seed, x, y)
    }
    fn cell(&self, seed: i32, x: i32, y: i32) -> CellPoint {
        (**self).cell(seed, x, y)
    }
    fn cell_shape(&self) -> CellShape {
        (**self).cell_shape()
    }
}

impl<Cells: ContinentCells + ?Sized> ContinentCells for std::sync::Arc<Cells> {
    fn nearest_cell(&self, seed: i32, x: f32, y: f32) -> (i32, i32) {
        (**self).nearest_cell(seed, x, y)
    }
    fn cell(&self, seed: i32, x: i32, y: i32) -> CellPoint {
        (**self).cell(seed, x, y)
    }
    fn cell_shape(&self) -> CellShape {
        (**self).cell_shape()
    }
}

#[derive(Clone, Copy)]
pub struct ShapeGenerator {
    base_falloff: f32,
    continent_falloff: f32,
    falloff_points: [FalloffPoint; 5],
    pub threshold: f32,
    pub base_falloff_min: f32,
    pub base_falloff_max: f32,
}

impl ShapeGenerator {
    pub fn new(config: ContinentConfig, control_points: ControlPoints) -> Self {
        Self {
            base_falloff: config.noise.base_noise_falloff,
            continent_falloff: config.noise.continent_noise_falloff,
            falloff_points: falloff_curve(control_points),
            threshold: config.shape.threshold,
            base_falloff_min: config.shape.threshold + config.shape.base_falloff_min,
            base_falloff_max: config.shape.threshold + config.shape.base_falloff_max,
        }
    }

    pub fn threshold_value(&self, cell: CellPoint) -> f32 {
        if cell.noise < self.threshold {
            0.0
        } else {
            1.0
        }
    }

    pub fn falloff(&self, continent_noise: f32) -> f32 {
        falloff(continent_noise, &self.falloff_points)
    }

    pub fn base_noise(&self, value: f32) -> f32 {
        let minimum = self.base_falloff_min;
        let maximum = self.base_falloff_max;
        let clamped = if value < minimum {
            minimum
        } else if value > maximum {
            maximum
        } else {
            value
        };
        let delta = clamped - minimum;
        let range = maximum - minimum;
        if delta > range { 1.0 } else { delta / range }
    }

    pub fn sample(
        &self,
        continent: &impl ContinentCells,
        seed: i32,
        x: f32,
        y: f32,
        sample: &mut NoiseSample,
    ) {
        let (center_x, center_y) = continent.nearest_cell(seed, x, y);
        let x = continent.cell_shape().adjust_x(x);
        let y = continent.cell_shape().adjust_y(y);
        let mut buffer = [(CellPoint::default(), 0.0); 25];
        let mut minimum0 = f32::MAX;
        let mut minimum1 = f32::MAX;
        for (index, local) in buffer.iter_mut().enumerate() {
            let cell_x = center_x.wrapping_add(index as i32 % 5 - 2);
            let cell_y = center_y.wrapping_add(index as i32 / 5 - 2);
            let cell = continent.cell(seed, cell_x, cell_y);
            let delta_x = x - cell.px;
            let delta_y = y - cell.py;
            let distance = ((delta_x * delta_x + delta_y * delta_y) as f64).sqrt() as f32;
            *local = (cell, distance);
            if distance < minimum0 {
                minimum1 = minimum0;
                minimum0 = distance;
            } else if distance < minimum1 {
                minimum1 = distance;
            }
        }
        self.sample_edges(minimum0, minimum1, &buffer, sample);
    }

    fn sample_edges(
        &self,
        minimum0: f32,
        minimum1: f32,
        buffer: &[(CellPoint, f32); 25],
        sample: &mut NoiseSample,
    ) {
        let border_distance = (minimum0 + minimum1) * 0.5;
        let base_blend = border_distance * self.base_falloff;
        let continent_blend = border_distance * self.continent_falloff;
        let mut sum_base = 0.0;
        let mut sum_continent = 0.0;
        let mut base_weights = 0.0;
        let mut continent_weights = 0.0;
        for &(cell, distance) in buffer {
            let base_weight = weight(distance, minimum0, base_blend);
            let continent_weight = weight(distance, minimum0, continent_blend);
            sum_base += cell.noise() * base_weight;
            sum_continent += self.threshold_value(cell) * continent_weight;
            base_weights += base_weight;
            continent_weights += continent_weight;
        }
        sample.base_noise = self.base_noise(sum_base / base_weights);
        sample.continent_noise = self.falloff(sum_continent / continent_weights);
    }
}

fn weight(distance: f32, origin: f32, blend_range: f32) -> f32 {
    let delta = distance - origin;
    if delta <= 0.0 {
        1.0
    } else if delta >= blend_range {
        0.0
    } else {
        1.0 - delta / blend_range
    }
}

#[cfg(test)]
mod tests {
    use super::super::terraforged_engine::WorldControlPoints;
    use super::*;
    use std::cell::RefCell;

    struct Cells {
        visits: RefCell<Vec<(i32, i32)>>,
        shape: CellShape,
    }

    impl ContinentCells for Cells {
        fn nearest_cell(&self, seed: i32, x: f32, y: f32) -> (i32, i32) {
            assert_eq!(seed, 42);
            assert_eq!((x, y), (0.25, -0.75));
            (0, -1)
        }

        fn cell(&self, seed: i32, x: i32, y: i32) -> CellPoint {
            assert_eq!(seed, 42);
            self.visits.borrow_mut().push((x, y));
            CellPoint {
                px: x as f32,
                py: y as f32,
                noise: 1.0,
                noise0: 0.0,
            }
        }

        fn cell_shape(&self) -> CellShape {
            self.shape
        }
    }

    #[test]
    fn sampling_preserves_order_and_other_sample_fields() {
        let generator = ShapeGenerator::new(
            ContinentConfig::default(),
            ControlPoints::new(WorldControlPoints::default()),
        );
        for shape in [CellShape::Square, CellShape::Hexagon] {
            let cells = Cells {
                visits: RefCell::default(),
                shape,
            };
            let mut sample = NoiseSample {
                height_noise: 0.123,
                river_noise: 0.456,
                continent_center: 789,
                ..NoiseSample::default()
            };
            generator.sample(&cells, 42, 0.25, -0.75, &mut sample);
            assert_eq!(sample.base_noise, 0.0);
            assert_eq!(sample.continent_noise, 1.0);
            assert_eq!(sample.height_noise, 0.123);
            assert_eq!(sample.river_noise, 0.456);
            assert_eq!(sample.continent_center, 789);
            let expected: Vec<_> = (-3..=1)
                .flat_map(|y| (-2..=2).map(move |x| (x, y)))
                .collect();
            assert_eq!(*cells.visits.borrow(), expected);
        }
    }

    #[test]
    fn blended_edges_match_java_shape_generator() {
        let generator = ShapeGenerator::new(
            ContinentConfig::default(),
            ControlPoints::new(WorldControlPoints::default()),
        );
        let buffer = std::array::from_fn(|index| {
            (
                CellPoint {
                    noise: (index % 3) as f32 * 0.3,
                    noise0: 0.6 + index as f32 * 0.025,
                    ..CellPoint::default()
                },
                0.25 + index as f32 * 0.125,
            )
        });
        for (fixture, (base, continent)) in [
            (0x3ebd097d, 0x3de38e38),
            (0x3f136dbf, 0x3e7ac686),
            (0x3f48a586, 0x3ea5c7ca),
        ]
        .into_iter()
        .enumerate()
        {
            let mut sample = NoiseSample::default();
            generator.sample_edges(0.25, 0.375 + fixture as f32, &buffer, &mut sample);
            assert_eq!(sample.base_noise.to_bits(), base);
            assert_eq!(sample.continent_noise.to_bits(), continent);
        }
    }

    #[test]
    fn weights_preserve_boundary_and_nan_behavior() {
        assert_eq!(weight(1.0, 1.0, 0.0), 1.0);
        assert_eq!(weight(2.0, 1.0, 0.0), 0.0);
        assert_eq!(weight(1.25, 1.0, 0.5), 0.5);
        assert!(weight(f32::NAN, 1.0, 0.5).is_nan());
    }
}
