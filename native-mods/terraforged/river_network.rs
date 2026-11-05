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
    terraforged::{hash, hash_2d, map},
    terraforged_continent_cell::CellPoint,
    terraforged_continent_config::ContinentConfig,
    terraforged_continent_shape::{ContinentCells, ShapeGenerator},
    terraforged_river_nodes::{RiverNode, RiverPieces},
    terraforged_terrain_blend_util::random,
};

/// `RiverGenerator.DIRS`: the four neighbours, in the order `computeNodes` visits them.
pub const DIRECTIONS: [(i32, i32); 4] = [(1, 0), (0, 1), (-1, 0), (0, -1)];

/// The newer source tree's `RiverGenerator.computeNodes` (`worldgen.noise.continent.river`): the river and lake
/// pieces of one continent cell. The 1.18.2-0.3.1 jar draws from `config.rivers.seed` and reads seedless cells; the
/// newer source (ported here) passes its runtime `seed` to both and never reads `rivers.seed`. Its nearest-node
/// sampling, river warp, carver call and piece cache are not ported here.
pub struct RiverNetwork<'a, Cells> {
    continent: &'a Cells,
    shape: &'a ShapeGenerator,
    lake_density: f32,
}

impl<'a, Cells: ContinentCells> RiverNetwork<'a, Cells> {
    /// `new RiverGenerator(continent, config)`, keeping only what `computeNodes` reads.
    pub fn new(continent: &'a Cells, shape: &'a ShapeGenerator, config: ContinentConfig) -> Self {
        Self {
            continent,
            shape,
            lake_density: config.rivers.lake_density,
        }
    }

    /// `computeNodes(seed, PosUtil.pack(cell_x, cell_y))`. An ocean cell has no pieces. A land cell gets the
    /// rivers of every higher neighbour whose lowest neighbour it is (the inflows, in [`DIRECTIONS`] order), then
    /// its own river to its lowest neighbour (the last one at or below its value; none if no neighbour is), and a
    /// lake when it is a source (no inflow) and [`Self::has_lake`]. A source whose lowest neighbour is ocean has
    /// no pieces at all. Neighbour coordinates wrap like Java `int`s.
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
                let cell_hash = hash_2d(seed.wrapping_add(827614), neighbor_x, neighbor_y);
                self.add_river_nodes(origin, neighbor, seed, cell_hash, &mut pieces);
                is_source = false;
            }
        }
        let Some(minimum) = minimum else {
            return pieces;
        };
        if is_source && pieces.rivers.is_empty() && minimum_value <= 0.0 {
            return RiverPieces::default();
        }
        let cell_hash = hash_2d(seed.wrapping_add(827614), cell_x, cell_y);
        self.add_river_nodes(origin, minimum, seed, cell_hash, &mut pieces);
        if is_source && self.has_lake(origin, cell_hash) {
            let size = (0.5 + random(hash(seed.wrapping_add(9803), cell_hash)) * 0.5) * 0.12;
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

    /// `addRiverNodes`: a segment from `origin` to a point displaced sideways off the first quarter of the
    /// connection, one on to the midpoint, and a third on to `target` when `target` is ocean (its `noise` field
    /// below the threshold). Heights are the cells' `noise()`, radii [`radius`] of it; the bend directions and
    /// amplitudes are drawn from `seed + 20107`, `seed + 9803` and `seed + 28387` with the connection's hash.
    fn add_river_nodes(
        &self,
        origin: CellPoint,
        target: CellPoint,
        seed: i32,
        cell_hash: i32,
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
        let direction = if random(hash(seed.wrapping_add(20107), cell_hash)) < 0.5 {
            -1.0
        } else {
            1.0
        };
        let amplitude0 = 0.7 + random(hash(seed.wrapping_add(9803), cell_hash)) * 0.3;
        let amplitude1 = 0.7 + random(hash(seed.wrapping_add(28387), cell_hash)) * 0.3;
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

    /// `connects`: whether `origin` is `target`'s strictly lowest neighbour. Java starts the running minimum at
    /// `(target.y, target.x)` (swapped), kept here; that start never equals `origin` (a neighbour differs from
    /// `target` by one in one coordinate), so without a strictly lower neighbour the answer is false either way.
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

    /// `hasLake`: the cell hash's draw at most the lake density, or a base noise below 0.25.
    fn has_lake(&self, cell: CellPoint, cell_hash: i32) -> bool {
        random(cell_hash.wrapping_add(37171)) <= self.lake_density
            || self.shape.base_noise(cell.noise()) < 0.25
    }

    /// `getBaseValue`: `noise()` on land, 0 in the ocean.
    fn base_value(&self, cell: CellPoint) -> f32 {
        if self.shape.threshold_value(cell) <= 0.0 {
            0.0
        } else {
            cell.noise()
        }
    }
}

/// `getRadius`: `1 - NoiseUtil.map(noise, 0.5, 0.7, 0.7 - 0.5)` (the range in `float`, as Java).
fn radius(noise: f32) -> f32 {
    1.0 - map(noise, 0.5, 0.7, 0.7 - 0.5)
}

/// Bit-exact tests against `RiverNetworkOracle.java` (`.local/terraforged/oracles/river_network/`), which runs
/// `RiverGenerator.computeNodes` of both the 1.18.2-0.3.1 jar and the newer source tree (they agree once the jar's
/// `rivers.seed` and shape seeds carry the runtime seed; see the README).
#[cfg(test)]
mod tests {
    use super::super::{
        terraforged_continent_cell::{CellShape, CellSource},
        terraforged_continent_cells::CellGenerator,
        terraforged_engine::{ControlPoints, WorldControlPoints},
    };
    use super::*;

    /// FNV-1a over 32-bit values, NaN canonicalised (Java's `floatToIntBits`), as the oracle.
    struct Fold {
        hash: u64,
        rivers: usize,
        lakes: usize,
    }

    impl Fold {
        fn new() -> Self {
            Self {
                hash: 0xcbf2_9ce4_8422_2325,
                rivers: 0,
                lakes: 0,
            }
        }

        fn add(&mut self, value: u64) {
            self.hash = (self.hash ^ value).wrapping_mul(0x0100_0000_01b3);
        }

        fn float(&mut self, value: f32) {
            self.add(if value.is_nan() {
                0x7fc0_0000
            } else {
                value.to_bits() as u64
            });
        }

        fn cell(&mut self, pieces: &RiverPieces) {
            self.add(pieces.rivers.len() as u64);
            self.add(pieces.lakes.len() as u64);
            self.rivers += pieces.rivers.len();
            self.lakes += pieces.lakes.len();
            for node in pieces.rivers.iter().chain(&pieces.lakes) {
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
                    self.float(value);
                }
            }
        }

        /// Checks and restarts the fold, like the oracle's `print`.
        fn check(&mut self, name: &str, expected: (usize, usize, u64)) {
            assert_eq!((self.rivers, self.lakes, self.hash), expected, "{name}");
            *self = Self::new();
        }
    }

    struct Setup {
        shape: CellShape,
        source: CellSource,
        seeds: (i32, i32),
        jitter: f32,
        threshold: f32,
        lake_density: f32,
    }

    impl Setup {
        fn new(shape: CellShape, source: CellSource) -> Self {
            Self {
                shape,
                source,
                seeds: (0, 0),
                jitter: 0.75,
                threshold: 0.525,
                lake_density: 0.75,
            }
        }

        /// Folds `computeNodes` at every cell of `xs` x `ys` (y outer, as the oracle).
        fn fold(&self, fold: &mut Fold, seed: i32, xs: &[i32], ys: &[i32]) {
            let mut config = ContinentConfig::default();
            config.shape.cell_shape = self.shape;
            config.shape.cell_source = self.source;
            (config.shape.seed0, config.shape.seed1) = self.seeds;
            config.shape.jitter = self.jitter;
            config.shape.threshold = self.threshold;
            config.rivers.lake_density = self.lake_density;
            config.rivers.seed = 0x5eed; // the newer source's network never reads it
            let cells = CellGenerator::new(config);
            let shape =
                ShapeGenerator::new(config, ControlPoints::new(WorldControlPoints::default()));
            let network = RiverNetwork::new(&cells, &shape, config);
            for &y in ys {
                for &x in xs {
                    fold.cell(&network.compute_nodes(seed, x, y));
                }
            }
        }
    }

    const SHAPES: [CellShape; 2] = CellShape::VALUES;
    const SOURCES: [CellSource; 3] = CellSource::VALUES;

    fn span(radius: i32) -> Vec<i32> {
        (-radius..=radius).collect()
    }

    #[test]
    fn network_grids_match_java() {
        let grid = [
            (284, 18, 0xa61e72e70bfd8701),
            (330, 31, 0x0ddc50bf2dc3f03d),
            (335, 26, 0xc4d1e7ebb817f20a),
            (274, 19, 0xde6b6d43da9e2abd),
            (318, 30, 0xc3dacc08116737f0),
            (331, 26, 0x58c2f1588e743a0e),
        ];
        let seeds = [
            (1754, 87, 0x96f1d9328dfe9bb7),
            (1918, 185, 0x7598072e0d602506),
            (2149, 117, 0xf255198dfdf3bca9),
            (1722, 93, 0x32b8403212da8806),
            (1877, 178, 0xf517ee3fa435d957),
            (2145, 128, 0x65c310fe931828ae),
        ];
        let extreme = [
            (15, 0, 0xf9f83309e1ae88aa),
            (10, 0, 0xb0a45d5d467190f5),
            (45, 6, 0xfb340da91a5cbb39),
            (15, 0, 0x0fc7083703ac686d),
            (10, 0, 0xd3dd463c17352526),
            (47, 5, 0x674f47cea50c0f06),
        ];
        const RUNTIME: [i32; 9] = [
            0,
            42,
            -1,
            i32::MAX,
            i32::MIN,
            -827_614,
            i32::MAX - 20_106,
            i32::MIN + 9_802,
            123_456_789,
        ];
        const BASE: [(i32, i32); 3] = [(0, 0), (i32::MAX, i32::MIN), (-6569, 987_654_321)];
        const EXTREME: [i32; 6] = [i32::MIN, i32::MIN + 1, -1, 0, i32::MAX - 1, i32::MAX];
        let mut fold = Fold::new();
        for (index, (shape, source)) in SHAPES
            .into_iter()
            .flat_map(|shape| SOURCES.map(|source| (shape, source)))
            .enumerate()
        {
            let mut setup = Setup::new(shape, source);
            setup.fold(&mut fold, 42, &span(6), &span(6));
            fold.check("grid", grid[index]);
            for seed in RUNTIME {
                for base in BASE {
                    setup.seeds = base;
                    setup.fold(&mut fold, seed, &span(3), &span(3));
                }
            }
            fold.check("seeds", seeds[index]);
            setup.seeds = (0, 0);
            setup.fold(&mut fold, 42, &EXTREME, &EXTREME);
            fold.check("extreme", extreme[index]);
        }
    }

    #[test]
    fn lake_density_threshold_and_jitter_edges_match_java() {
        let mut fold = Fold::new();
        for lake_density in [
            0.0,
            1.0,
            -0.0,
            f32::NAN,
            f32::NEG_INFINITY,
            f32::INFINITY,
            0.25,
        ] {
            let mut setup = Setup::new(CellShape::Square, CellSource::Perlin);
            setup.lake_density = lake_density;
            setup.fold(&mut fold, 42, &span(6), &span(6));
        }
        fold.check("lakes", (1988, 72, 0xacd3c4dbe7195797));
        for threshold in [
            0.0,
            0.3,
            0.7,
            1.0,
            -0.0,
            f32::NAN,
            f32::NEG_INFINITY,
            f32::INFINITY,
        ] {
            let mut setup = Setup::new(CellShape::Hexagon, CellSource::Simplex);
            setup.threshold = threshold;
            setup.fold(&mut fold, 42, &span(6), &span(6));
        }
        fold.check("thresholds", (3221, 213, 0x36c6d5cf1f65a35f));
        for jitter in [
            0.0,
            1.0,
            -1.0,
            f32::from_bits(1),
            1e30,
            f32::NAN,
            f32::INFINITY,
        ] {
            let mut setup = Setup::new(CellShape::Square, CellSource::Cubic);
            setup.jitter = jitter;
            setup.fold(&mut fold, 42, &span(6), &span(6));
        }
        fold.check("jitters", (1334, 94, 0x514a6f690352c7e7));
    }
}
