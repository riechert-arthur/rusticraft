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
        DistanceFunction, Domain, EdgeFunction, Module, cell_vector, floor, noise_map,
        value_coord_2d,
    },
    terraforged_engine::Cell,
    terraforged_populator::Populator,
};

#[derive(Clone)]
pub struct RegionConfig {
    pub seed: i32,
    pub scale: i32,
    pub warp_x: Module,
    pub warp_z: Module,
    pub warp_strength: f64,
}

impl RegionConfig {
    pub fn new(seed: i32, scale: i32, warp_x: Module, warp_z: Module, warp_strength: f64) -> Self {
        Self {
            seed,
            scale,
            warp_x,
            warp_z,
            warp_strength,
        }
    }
}

pub struct RegionModule {
    seed: i32,
    frequency: f32,
    edge_min: f32,
    edge_max: f32,
    edge_range: f32,
    warp: Domain,
}

impl RegionModule {
    pub fn new(config: RegionConfig) -> Self {
        let edge_min = 0.0;
        let edge_max = 0.5;
        Self {
            seed: config.seed.wrapping_add(7),
            frequency: 1.0 / config.scale as f32,
            edge_min,
            edge_max,
            edge_range: edge_max - edge_min,
            warp: Domain::domain_warp(
                config.warp_x,
                config.warp_z,
                Module::constant(config.warp_strength as f32),
            ),
        }
    }

    fn cell_value(&self, cell_x: i32, cell_y: i32) -> f32 {
        noise_map(value_coord_2d(self.seed, cell_x, cell_y), -1.0, 1.0, 2.0)
    }

    fn edge_value(&self, distance: f32, distance2: f32) -> f32 {
        let edge = EdgeFunction::Distance2Div;
        let value = edge.apply(distance, distance2);
        let (min, max, range) = edge.bounds();
        let mut edge_value = 1.0 - noise_map(value, min, max, range);
        edge_value = f64::from(edge_value).powf(1.5) as f32;
        if edge_value < self.edge_min {
            return 0.0;
        }
        if edge_value > self.edge_max {
            return 1.0;
        }
        (edge_value - self.edge_min) / self.edge_range
    }
}

impl Populator for RegionModule {
    fn apply(&self, cell: &mut Cell, x: f32, y: f32) {
        let mut point_x = x + self.warp.offset_x(x, y);
        let mut point_y = y + self.warp.offset_y(x, y);
        point_x *= self.frequency;
        point_y *= self.frequency;
        let mut cell_x = 0;
        let mut cell_y = 0;
        let mut center_x = 0.0;
        let mut center_y = 0.0;
        let x_index = floor(point_x);
        let y_index = floor(point_y);
        let mut edge_distance = f32::MAX;
        let mut edge_distance2 = f32::MAX;
        for delta_y in -1_i32..=1 {
            for delta_x in -1_i32..=1 {
                let candidate_x = x_index.wrapping_add(delta_x);
                let candidate_y = y_index.wrapping_add(delta_y);
                let vector = cell_vector(self.seed, candidate_x, candidate_y);
                let vector_x = candidate_x as f32 + vector[0] * 0.7;
                let vector_y = candidate_y as f32 + vector[1] * 0.7;
                let distance =
                    DistanceFunction::Natural.apply(vector_x - point_x, vector_y - point_y);
                if distance < edge_distance {
                    edge_distance2 = edge_distance;
                    edge_distance = distance;
                    center_x = vector_x;
                    center_y = vector_y;
                    cell_x = candidate_x;
                    cell_y = candidate_y;
                } else if distance < edge_distance2 {
                    edge_distance2 = distance;
                }
            }
        }
        cell.terrain_region_id = self.cell_value(cell_x, cell_y);
        cell.terrain_region_edge = self.edge_value(edge_distance, edge_distance2);
        cell.terrain_region_center = pack(center_x / self.frequency, center_y / self.frequency);
    }
}

fn pack(left: f32, right: f32) -> i64 {
    let left = left as i32;
    let right = right as i32;
    (i64::from(left) << 32) | (i64::from(right) & 0xffff_ffff)
}

#[cfg(test)]
mod tests {
    use super::super::terraforged::{Interpolation, Source, SourceConfig, SourceKind};
    use super::*;

    fn perlin(seed: i32, scale: f32, octaves: usize) -> Module {
        Module::source(Source::new(
            SourceKind::Perlin,
            SourceConfig {
                seed,
                octaves,
                gain: 0.5,
                frequency: 1.0 / scale,
                lacunarity: 2.0,
                interpolation: Interpolation::Curve3,
            },
        ))
    }

    fn sample(module: &RegionModule) -> [(u32, u32, u64); 4] {
        [
            (0.0, 0.0),
            (12.25, -98.75),
            (-4096.5, 8192.25),
            (123456.75, -0.125),
        ]
        .map(|(x, y)| {
            let mut cell = Cell::default();
            module.apply(&mut cell, x, y);
            (
                cell.terrain_region_id.to_bits(),
                cell.terrain_region_edge.to_bits(),
                cell.terrain_region_center as u64,
            )
        })
    }

    #[test]
    fn region_module_matches_engine_0_3_0() {
        let module = RegionModule::new(RegionConfig::new(
            123,
            400,
            perlin(7, 100.0, 2),
            perlin(8, 100.0, 2),
            50.5,
        ));
        assert_eq!(
            sample(&module),
            [
                (0x3ee34961, 0x3f0b09d3, 0x0000008a0000000e),
                (0x3ee34961, 0x3ec24129, 0x0000008a0000000e),
                (0x3f430bf0, 0x3f51cfb3, 0xfffff06f00001fd0),
                (0x3e9df004, 0x3e1200fc, 0x0001e308ffffff5a),
            ]
        );

        let overflow = RegionModule::new(RegionConfig::new(
            i32::MAX - 3,
            37,
            perlin(-99, 17.0, 3),
            perlin(-98, 17.0, 3),
            -12.25,
        ));
        assert_eq!(
            sample(&overflow),
            [
                (0x3ea6991c, 0x3d8d04ae, 0x00000005fffffff1),
                (0x3f1b511e, 0x3f0cbc32, 0x00000007ffffffa9),
                (0x3f238519, 0x3f3aa52b, 0xfffff00e00001ffc),
                (0x3e2dd120, 0x3d843f55, 0x0001e23afffffff3),
            ]
        );
    }
}
