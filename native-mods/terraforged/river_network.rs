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
    terraforged::hash_2d,
    terraforged_continent_cell::{CellPoint, hash_1d, random},
    terraforged_continent_config::ContinentConfig,
    terraforged_continent_shape::{ContinentCells, ShapeGenerator},
    terraforged_river_nodes::{CarverSample, NodeSample, RiverNode, RiverPieces, map},
};

pub const DIRECTIONS: [(i32, i32); 4] = [(1, 0), (0, 1), (-1, 0), (0, -1)];

pub struct RiverNetwork<'a, Cells> {
    continent: &'a Cells,
    shape: &'a ShapeGenerator,
    lake_density: f32,
}

impl<'a, Cells: ContinentCells> RiverNetwork<'a, Cells> {
    pub fn new(continent: &'a Cells, shape: &'a ShapeGenerator, config: ContinentConfig) -> Self {
        Self {
            continent,
            shape,
            lake_density: config.rivers.lake_density,
        }
    }

    pub fn compute_nodes(&self, seed: i32, cell_x: i32, cell_y: i32) -> RiverPieces {
        let origin = self.continent.cell(seed, cell_x, cell_y);
        if self.shape.threshold_value(origin) <= 0.0 {
            return RiverPieces::default();
        }
        let mut minimum = None;
        let mut minimum_value = self.base_value(origin);
        let mut is_source = true;
        let mut pieces = RiverPieces::default();
        for (delta_x, delta_y) in DIRECTIONS {
            let neighbor_x = cell_x.wrapping_add(delta_x);
            let neighbor_y = cell_y.wrapping_add(delta_y);
            let neighbor = self.continent.cell(seed, neighbor_x, neighbor_y);
            let value = self.base_value(neighbor);
            if value <= minimum_value {
                minimum = Some(neighbor);
                minimum_value = value;
                continue;
            }
            if value <= 0.0 {
                continue;
            }
            if self.connects(seed, (cell_x, cell_y), (neighbor_x, neighbor_y), value) {
                let hash = hash_2d(seed.wrapping_add(827614), neighbor_x, neighbor_y);
                self.add_river_nodes(origin, neighbor, seed, hash, &mut pieces);
                is_source = false;
            }
        }
        let Some(minimum) = minimum else {
            return pieces;
        };
        if is_source && pieces.rivers.is_empty() && minimum_value <= 0.0 {
            return RiverPieces::default();
        }
        let hash = hash_2d(seed.wrapping_add(827614), cell_x, cell_y);
        self.add_river_nodes(origin, minimum, seed, hash, &mut pieces);
        if is_source && self.has_lake(origin, hash) {
            let size = (0.5 + random(hash_1d(seed.wrapping_add(9803), hash)) * 0.5) * 0.12;
            let delta_x = origin.px - minimum.px;
            let delta_y = origin.py - minimum.py;
            pieces.lakes.push(RiverNode {
                ax: origin.px,
                ay: origin.py,
                bx: origin.px + delta_x * size,
                by: origin.py + delta_y * size,
                ah: origin.noise(),
                bh: origin.noise(),
                ar: 1.0,
                br: 1.0,
                displacement: 0.0,
            });
        }
        pieces
    }

    pub fn sample_nodes(&self, seed: i32, x: f32, y: f32, sample: &mut CarverSample) {
        sample.reset();
        let (center_x, center_y) = self.continent.nearest_cell(seed, x, y);
        let x = self.continent.cell_shape().adjust_x(x);
        let y = self.continent.cell_shape().adjust_y(y);
        let mut river = None;
        let mut lake = None;
        for delta_y in -1..=1 {
            for delta_x in -1..=1 {
                let pieces = self.compute_nodes(
                    seed,
                    center_x.wrapping_add(delta_x),
                    center_y.wrapping_add(delta_y),
                );
                for node in pieces.rivers {
                    sample_node(x, y, node, &mut river, &mut sample.river);
                }
                for node in pieces.lakes {
                    sample_node(x, y, node, &mut lake, &mut sample.lake);
                }
            }
        }
        self.record_node(river, &mut sample.river);
        self.record_node(lake, &mut sample.lake);
    }

    fn record_node(&self, node: Option<RiverNode>, sample: &mut NodeSample) {
        if let Some(node) = node {
            let level = node.height(sample.projection);
            let radius = node.radius(sample.projection);
            sample.distance = (sample.distance as f64).sqrt() as f32;
            sample.position = radius;
            sample.level = self.shape.base_noise(level);
        } else {
            sample.invalidate();
        }
    }

    fn add_river_nodes(
        &self,
        origin: CellPoint,
        target: CellPoint,
        seed: i32,
        hash: i32,
        pieces: &mut RiverPieces,
    ) {
        let origin_height = origin.noise();
        let target_height = target.noise();
        let origin_radius = radius(origin.noise());
        let target_radius = radius(target.noise());
        let middle_x = (origin.px + target.px) * 0.5;
        let middle_y = (origin.py + target.py) * 0.5;
        let middle_radius = (origin_radius + target_radius) * 0.5;
        let middle_height = (origin_height + target_height) * 0.5;
        let mut center_x = (origin.px + middle_x) * 0.5;
        let mut center_y = (origin.py + middle_y) * 0.5;
        let center_radius = (origin_radius + middle_radius) * 0.5;
        let center_height = (origin_height + middle_height) * 0.5;
        let normal_x = -(center_y - origin.py);
        let normal_y = center_x - origin.px;
        let direction = if random(hash_1d(seed.wrapping_add(20107), hash)) < 0.5 {
            -1.0
        } else {
            1.0
        };
        let amplitude0 = 0.7 + random(hash_1d(seed.wrapping_add(9803), hash)) * 0.3;
        let amplitude1 = 0.7 + random(hash_1d(seed.wrapping_add(28387), hash)) * 0.3;
        let displacement = 0.35 * direction * amplitude0;
        center_x += normal_x * displacement;
        center_y += normal_y * displacement;
        let strength = 0.275 * -direction * amplitude1;
        let warp1 = strength * map(origin.noise, 0.4, 0.6, 0.2);
        let warp2 = -strength * map(target.noise, 0.4, 0.6, 0.2);
        pieces.rivers.push(RiverNode {
            ax: origin.px,
            ay: origin.py,
            bx: center_x,
            by: center_y,
            ah: origin_height,
            bh: center_height,
            ar: origin_radius,
            br: center_radius,
            displacement: warp1,
        });
        pieces.rivers.push(RiverNode {
            ax: center_x,
            ay: center_y,
            bx: middle_x,
            by: middle_y,
            ah: center_height,
            bh: middle_height,
            ar: center_radius,
            br: middle_radius,
            displacement: warp2,
        });
        if target.noise < self.shape.threshold {
            pieces.rivers.push(RiverNode {
                ax: middle_x,
                ay: middle_y,
                bx: target.px,
                by: target.py,
                ah: middle_height,
                bh: target_height,
                ar: middle_radius,
                br: target_radius,
                displacement: warp1,
            });
        }
    }

    fn connects(
        &self,
        seed: i32,
        origin: (i32, i32),
        target: (i32, i32),
        mut minimum_value: f32,
    ) -> bool {
        let mut minimum_y = target.0;
        let mut minimum_x = target.1;
        for (delta_x, delta_y) in DIRECTIONS {
            let cell_x = target.0.wrapping_add(delta_x);
            let cell_y = target.1.wrapping_add(delta_y);
            let cell = self.continent.cell(seed, cell_x, cell_y);
            let value = self.base_value(cell);
            if value < minimum_value {
                minimum_x = cell_x;
                minimum_y = cell_y;
                minimum_value = value;
            }
        }
        (minimum_x, minimum_y) == origin
    }

    fn has_lake(&self, cell: CellPoint, hash: i32) -> bool {
        random(hash.wrapping_add(37171)) <= self.lake_density
            || self.shape.base_noise(cell.noise()) < 0.25
    }

    fn base_value(&self, cell: CellPoint) -> f32 {
        if self.shape.threshold_value(cell) <= 0.0 {
            0.0
        } else {
            cell.noise()
        }
    }
}

fn sample_node(
    x: f32,
    y: f32,
    node: RiverNode,
    nearest: &mut Option<RiverNode>,
    sample: &mut NodeSample,
) {
    let projection = node.projection(x, y);
    let distance = node.distance_squared(x, y, projection);
    if distance < sample.distance {
        *nearest = Some(node);
        sample.distance = distance;
        sample.projection = projection;
    }
}

fn radius(noise: f32) -> f32 {
    1.0 - map(noise, 0.5, 0.7, 0.7 - 0.5)
}

#[cfg(test)]
mod tests {
    use super::super::{
        terraforged_continent_cell::{CellShape, CellSource},
        terraforged_continent_cells::CellGenerator,
        terraforged_engine::{ControlPoints, TerrainRegistry, WorldControlPoints},
    };
    use super::*;

    fn add(hash: u64, value: u64) -> u64 {
        (hash ^ value).wrapping_mul(1099511628211)
    }

    fn hash_node(mut hash: u64, node: &RiverNode) -> u64 {
        for value in [
            node.ax,
            node.ay,
            node.bx,
            node.by,
            node.ah,
            node.bh,
            node.ar,
            node.br,
            node.displacement,
        ] {
            hash = add(hash, value.to_bits() as u64);
        }
        hash
    }

    #[test]
    fn sampling_grids_match_current_java_source() {
        let expected = [
            (0x52afc5b1e3a9ae12, 48),
            (0x05fddf6070b941a3, 33),
            (0x92698081257698e4, 16),
            (0x1b335ab677b76f83, 49),
            (0x53e66569aa7e1d11, 33),
            (0xf9e6c2ecefc8b5e9, 19),
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
                let cells = CellGenerator::new(config);
                let shape =
                    ShapeGenerator::new(config, ControlPoints::new(WorldControlPoints::default()));
                let network = RiverNetwork::new(&cells, &shape, config);
                let mut sample = CarverSample::new(&TerrainRegistry::default());
                let mut hash = 0xcbf29ce484222325;
                let mut invalid = 0;
                for sample_y in -3..=3 {
                    for sample_x in -3..=3 {
                        network.sample_nodes(
                            42,
                            sample_x as f32 + 0.125,
                            sample_y as f32 - 0.375,
                            &mut sample,
                        );
                        for node in [&sample.river, &sample.lake] {
                            invalid += usize::from(node.is_invalid());
                            for value in [node.projection, node.distance, node.position, node.level]
                            {
                                let bits = if value.is_nan() {
                                    0x7fc00000
                                } else {
                                    value.to_bits()
                                };
                                hash = add(hash, bits as u64);
                            }
                        }
                    }
                }
                assert_eq!((hash, invalid), expected[shape_index * 3 + source_index]);
            }
        }
    }

    #[test]
    fn equal_distance_keeps_first_node() {
        let first = RiverNode {
            ax: 0.0,
            ay: 0.0,
            bx: 1.0,
            by: 0.0,
            ah: 0.2,
            bh: 0.3,
            ar: 0.5,
            br: 1.0,
            displacement: 0.0,
        };
        let second = RiverNode { ah: 0.9, ..first };
        let mut samples = CarverSample::new(&TerrainRegistry::default());
        samples.reset();
        let mut nearest = None;
        sample_node(0.5, 0.25, first, &mut nearest, &mut samples.river);
        sample_node(0.5, 0.25, second, &mut nearest, &mut samples.river);
        assert_eq!(nearest, Some(first));
        assert_eq!(samples.river.distance, 0.0625);
        assert_eq!(samples.river.projection, 0.5);
    }

    #[test]
    fn network_grids_match_current_java_source() {
        let expected = [
            (0xa61e72e70bfd8701, 284, 18),
            (0x0ddc50bf2dc3f03d, 330, 31),
            (0xc4d1e7ebb817f20a, 335, 26),
            (0xde6b6d43da9e2abd, 274, 19),
            (0xc3dacc08116737f0, 318, 30),
            (0x58c2f1588e743a0e, 331, 26),
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
                let cells = CellGenerator::new(config);
                let shape =
                    ShapeGenerator::new(config, ControlPoints::new(WorldControlPoints::default()));
                let network = RiverNetwork::new(&cells, &shape, config);
                let mut hash = 0xcbf29ce484222325;
                let mut rivers = 0;
                let mut lakes = 0;
                for cell_y in -6..=6 {
                    for cell_x in -6..=6 {
                        let pieces = network.compute_nodes(42, cell_x, cell_y);
                        hash = add(hash, pieces.rivers.len() as u64);
                        hash = add(hash, pieces.lakes.len() as u64);
                        rivers += pieces.rivers.len();
                        lakes += pieces.lakes.len();
                        for node in pieces.rivers.iter().chain(pieces.lakes.iter()) {
                            hash = hash_node(hash, node);
                        }
                    }
                }
                assert_eq!(
                    (hash, rivers, lakes),
                    expected[shape_index * 3 + source_index]
                );
            }
        }
    }
}
