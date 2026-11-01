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
    terraforged::{
        DistanceFunction, Domain, EdgeFunction, Interpolation, Module, Source, SourceConfig,
        SourceKind, cell_vector, floor, noise_map, value_coord_2d,
    },
    terraforged_engine::{Cell, ContinentShape, ControlPoints, Seed, WorldSettings},
    terraforged_populator::Populator,
    terraforged_util::{pack, unpack_left, unpack_right},
};

pub struct SimpleContinentGenerator {
    seed: i32,
    frequency: f32,
    continent_scale: i32,
    distance_function: DistanceFunction,
    control_points: ControlPoints,
    clamp_min: f32,
    clamp_max: f32,
    clamp_range: f32,
    offset_alpha: f32,
    warp: Domain,
    shape: Module,
    single_center: Option<(i32, i32)>,
}

impl SimpleContinentGenerator {
    pub fn multi(seed: &mut Seed, settings: WorldSettings) -> Self {
        Self::new(seed, settings, false)
    }

    pub fn single(seed: &mut Seed, settings: WorldSettings) -> Self {
        Self::new(seed, settings, true)
    }

    fn new(seed: &mut Seed, settings: WorldSettings, single: bool) -> Self {
        let tectonic_scale = settings.continent.continent_scale.wrapping_mul(4);
        let continent_scale = settings.continent.continent_scale / 2;
        let generator_seed = seed.next();
        let distance_function = match settings.continent.continent_shape {
            ContinentShape::Euclidean => DistanceFunction::Euclidean,
            ContinentShape::Manhattan => DistanceFunction::Manhattan,
            ContinentShape::Natural => DistanceFunction::Natural,
        };
        let first_warp = noise_warp(SourceKind::Perlin, seed.next(), 20, 2, 20.0);
        let second_warp = noise_warp(
            SourceKind::Simplex,
            seed.next(),
            continent_scale,
            3,
            continent_scale as f32,
        );
        let shape = source(
            SourceKind::Simplex,
            seed.next(),
            settings.continent.continent_scale.wrapping_mul(2),
            1,
        )
        .bias(Module::constant(0.65))
        .clamp(Module::constant(0.0), Module::constant(1.0));
        let mut generator = Self {
            seed: generator_seed,
            frequency: 1.0 / tectonic_scale as f32,
            continent_scale,
            distance_function,
            control_points: ControlPoints::new(settings.control_points),
            clamp_min: 0.2,
            clamp_max: 1.0,
            clamp_range: 0.8,
            offset_alpha: settings.continent.continent_jitter,
            warp: first_warp.compound(second_warp),
            shape,
            single_center: None,
        };
        if single {
            let center = generator.nearest_center(0.0, 0.0);
            generator.single_center = Some((unpack_left(center), unpack_right(center)));
        }
        generator
    }

    pub fn edge_value(&self, x: f32, y: f32) -> f32 {
        let (point_x, point_y) = self.warped_point(x, y);
        let x_index = floor(point_x);
        let y_index = floor(point_y);
        let mut edge_distance = 999_999.0;
        let mut edge_distance2 = 999_999.0;
        for delta_y in -1_i32..=1 {
            for delta_x in -1_i32..=1 {
                let cell_x = x_index.wrapping_add(delta_x);
                let cell_y = y_index.wrapping_add(delta_y);
                let vector = cell_vector(self.seed, cell_x, cell_y);
                let center_x = cell_x as f32 + vector[0] * self.offset_alpha;
                let center_y = cell_y as f32 + vector[1] * self.offset_alpha;
                let distance = self
                    .distance_function
                    .apply(center_x - point_x, center_y - point_y);
                if distance < edge_distance {
                    edge_distance2 = edge_distance;
                    edge_distance = distance;
                } else if distance < edge_distance2 {
                    edge_distance2 = distance;
                }
            }
        }
        let edge = self.cell_edge_value(edge_distance, edge_distance2);
        edge * self.shape_value(x, y, edge)
    }

    pub fn land_value(&self, x: f32, z: f32) -> f32 {
        self.edge_value(x, z)
    }

    pub fn nearest_center(&self, x: f32, z: f32) -> i64 {
        let (point_x, point_y) = self.warped_point(x, z);
        let mut center_x = point_x;
        let mut center_y = point_y;
        let x_index = floor(point_x);
        let y_index = floor(point_y);
        let mut edge_distance = 999_999.0;
        for delta_y in -1_i32..=1 {
            for delta_x in -1_i32..=1 {
                let cell_x = x_index.wrapping_add(delta_x);
                let cell_y = y_index.wrapping_add(delta_y);
                let vector = cell_vector(self.seed, cell_x, cell_y);
                let candidate_x = cell_x as f32 + vector[0] * self.offset_alpha;
                let candidate_y = cell_y as f32 + vector[1] * self.offset_alpha;
                let distance = self
                    .distance_function
                    .apply(candidate_x - point_x, candidate_y - point_y);
                if distance < edge_distance {
                    edge_distance = distance;
                    center_x = candidate_x;
                    center_y = candidate_y;
                }
            }
        }
        pack(
            (center_x / self.frequency) as i32,
            (center_y / self.frequency) as i32,
        )
    }

    pub fn distance_to_ocean(&self, center_x: i32, center_z: i32, dx: f32, dz: f32) -> f32 {
        let mut high = self.distance_to_edge(center_x, center_z, dx, dz);
        let mut low = 0.0;
        for _ in 0..50 {
            let mid = (low + high) / 2.0;
            let x = center_x as f32 + dx * mid;
            let z = center_z as f32 + dz * mid;
            if self.edge_value(x, z) > self.control_points.shallow_ocean {
                low = mid;
            } else {
                high = mid;
            }
            if high - low < 10.0 {
                break;
            }
        }
        high
    }

    pub fn distance_to_edge(&self, center_x: i32, center_z: i32, dx: f32, dz: f32) -> f32 {
        let mut distance = self.continent_scale.wrapping_mul(4) as f32;
        for _ in 0..10 {
            let x = center_x as f32 + dx * distance;
            let z = center_z as f32 + dz * distance;
            let center = self.nearest_center(x, z);
            if unpack_left(center) == center_x && unpack_right(center) == center_z {
                distance += distance;
                continue;
            }
            let mut low = 0.0;
            let mut high = distance;
            for _ in 0..50 {
                let mid = (low + high) / 2.0;
                let point_x = center_x as f32 + dx * mid;
                let point_z = center_z as f32 + dz * mid;
                let center = self.nearest_center(point_x, point_z);
                if unpack_left(center) == center_x && unpack_right(center) == center_z {
                    low = mid;
                } else {
                    high = mid;
                }
                if high - low < 50.0 {
                    break;
                }
            }
            return high;
        }
        distance
    }

    fn warped_point(&self, x: f32, y: f32) -> (f32, f32) {
        (
            (x + self.warp.offset_x(x, y)) * self.frequency,
            (y + self.warp.offset_y(x, y)) * self.frequency,
        )
    }

    fn cell_identity(&self, cell_x: i32, cell_y: i32) -> f32 {
        noise_map(value_coord_2d(self.seed, cell_x, cell_y), -1.0, 1.0, 2.0)
    }

    fn cell_edge_value(&self, distance: f32, distance2: f32) -> f32 {
        let edge = EdgeFunction::Distance2Div;
        let (min, max, range) = edge.bounds();
        let value = 1.0 - noise_map(edge.apply(distance, distance2), min, max, range);
        if value <= self.clamp_min {
            0.0
        } else if value >= self.clamp_max {
            1.0
        } else {
            (value - self.clamp_min) / self.clamp_range
        }
    }

    fn shape_value(&self, x: f32, z: f32, edge: f32) -> f32 {
        if edge >= self.control_points.inland {
            1.0
        } else {
            self.shape.value(x, z) * (edge / self.control_points.inland)
        }
    }
}

impl Populator for SimpleContinentGenerator {
    fn apply(&self, cell: &mut Cell, x: f32, y: f32) {
        let (point_x, point_y) = self.warped_point(x, y);
        let x_index = floor(point_x);
        let y_index = floor(point_y);
        let mut cell_x = x_index;
        let mut cell_y = y_index;
        let mut center_x = point_x;
        let mut center_y = point_y;
        let mut edge_distance = 999_999.0;
        let mut edge_distance2 = 999_999.0;
        for delta_y in -1_i32..=1 {
            for delta_x in -1_i32..=1 {
                let candidate_x = x_index.wrapping_add(delta_x);
                let candidate_y = y_index.wrapping_add(delta_y);
                let vector = cell_vector(self.seed, candidate_x, candidate_y);
                let candidate_center_x = candidate_x as f32 + vector[0] * self.offset_alpha;
                let candidate_center_y = candidate_y as f32 + vector[1] * self.offset_alpha;
                let distance = self
                    .distance_function
                    .apply(candidate_center_x - point_x, candidate_center_y - point_y);
                if distance < edge_distance {
                    edge_distance2 = edge_distance;
                    edge_distance = distance;
                    center_x = candidate_center_x;
                    center_y = candidate_center_y;
                    cell_x = candidate_x;
                    cell_y = candidate_y;
                } else if distance < edge_distance2 {
                    edge_distance2 = distance;
                }
            }
        }
        cell.continent_id = self.cell_identity(cell_x, cell_y);
        cell.continent_edge = self.cell_edge_value(edge_distance, edge_distance2);
        cell.continent_x = (center_x / self.frequency) as i32;
        cell.continent_z = (center_y / self.frequency) as i32;
        cell.continent_edge *= self.shape_value(x, y, cell.continent_edge);
        if let Some(center) = self.single_center {
            if (cell.continent_x, cell.continent_z) != center {
                cell.continent_id = 0.0;
                cell.continent_edge = 0.0;
                cell.continent_x = 0;
                cell.continent_z = 0;
            }
        }
    }

    fn value(&self, x: f32, z: f32) -> f32 {
        self.edge_value(x, z)
    }
}

fn source(kind: SourceKind, seed: i32, scale: i32, octaves: usize) -> Module {
    Module::source(Source::new(
        kind,
        SourceConfig {
            seed,
            octaves,
            gain: 0.5,
            frequency: 1.0 / scale as f32,
            lacunarity: 2.0,
            interpolation: Interpolation::Curve3,
        },
    ))
}

fn noise_warp(kind: SourceKind, seed: i32, scale: i32, octaves: usize, strength: f32) -> Domain {
    Domain::domain_warp(
        source(kind, seed, scale, octaves),
        source(kind, seed.wrapping_add(1), scale, octaves),
        Module::constant(strength),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn samples(generator: &SimpleContinentGenerator) -> [(u32, u32, i32, i32, u64, u32); 4] {
        [
            (0.0, 0.0),
            (12.25, -98.75),
            (-4096.5, 8192.25),
            (123456.75, -0.125),
        ]
        .map(|(x, z)| {
            let mut cell = Cell::default();
            generator.apply(&mut cell, x, z);
            (
                cell.continent_id.to_bits(),
                cell.continent_edge.to_bits(),
                cell.continent_x,
                cell.continent_z,
                generator.nearest_center(x, z) as u64,
                generator.edge_value(x, z).to_bits(),
            )
        })
    }

    #[test]
    fn simple_continents_match_engine_0_3_0() {
        let settings = WorldSettings {
            seed: 246_813_579,
            ..WorldSettings::default()
        };
        let mut seed = Seed::new(12_345);
        let multi = SimpleContinentGenerator::multi(&mut seed, settings);
        assert_eq!(seed.get(), 12_349);
        assert_eq!(
            samples(&multi),
            [
                (0x3f504337, 0, 538, -8737, 0x0000021affffdddf, 0),
                (
                    0x3f504337,
                    0x3c5b6cf7,
                    538,
                    -8737,
                    0x0000021affffdddf,
                    0x3c5b6cf7
                ),
                (
                    0x3f16fc4e,
                    0x3cc74025,
                    -11502,
                    4962,
                    0xffffd31200001362,
                    0x3cc74025
                ),
                (
                    0x3b40d400,
                    0x3f19e30a,
                    127451,
                    2273,
                    0x0001f1db000008e1,
                    0x3f19e30a
                ),
            ]
        );
        let center = multi.nearest_center(0.0, 0.0);
        assert_eq!(
            multi
                .distance_to_edge(unpack_left(center), unpack_right(center), 0.6, 0.8)
                .to_bits(),
            0x46060880
        );
        assert_eq!(
            multi
                .distance_to_ocean(unpack_left(center), unpack_right(center), 0.6, 0.8)
                .to_bits(),
            0x45da53d8
        );

        let mut seed = Seed::new(12_345);
        let single = SimpleContinentGenerator::single(&mut seed, settings);
        assert_eq!(
            samples(&single),
            [
                (0x3f504337, 0, 538, -8737, 0x0000021affffdddf, 0),
                (
                    0x3f504337,
                    0x3c5b6cf7,
                    538,
                    -8737,
                    0x0000021affffdddf,
                    0x3c5b6cf7
                ),
                (0, 0, 0, 0, 0xffffd31200001362, 0x3cc74025),
                (0, 0, 0, 0, 0x0001f1db000008e1, 0x3f19e30a),
            ]
        );
    }
}
