/*
 * MIT License
 *
 * Copyright (c) 2021 TerraForged
 * Copyright (c) 2020 TerraForged
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
    terraforged::{Source, SourceConfig, SourceKind, hash, hash_2d, map, map_current},
    terraforged_continent_carver::RiverCarver,
    terraforged_continent_cell::CellPoint,
    terraforged_continent_cells::window,
    terraforged_continent_config::ContinentConfig,
    terraforged_continent_shape::{ContinentCells, ShapeGenerator},
    terraforged_engine::TerrainRegistry,
    terraforged_lossy_cache::ConcurrentLossyCache,
    terraforged_river_nodes::{CarverSample, NodeSample, RiverNode, RiverPieces},
    terraforged_terrain_blend_util::random,
    terraforged_terrain_data::NoiseSample,
    terraforged_terrain_generator::available_processors,
    terraforged_util::{pack, unpack_left, unpack_right},
};
use std::sync::Arc;

/// `RiverGenerator.RIVER_CACHE_SIZE`: the piece cache's capacity.
const RIVER_CACHE_SIZE: i32 = 1024;

/// `RiverGenerator.DIRS`: the four neighbours, in the order `computeNodes` visits them.
pub const DIRECTIONS: [(i32, i32); 4] = [(1, 0), (0, 1), (-1, 0), (0, -1)];

/// The newer source tree's `RiverGenerator.computeNodes` (`worldgen.noise.continent.river`): the river and lake
/// pieces of one continent cell. The 1.18.2-0.3.1 jar draws from `config.rivers.seed` and reads seedless cells; the
/// newer source (ported here) passes its runtime `seed` to both and never reads `rivers.seed`. [`Self::sample`] is
/// the warped, carved `sample`; [`Self::nodes`] is `getNodes`, the piece cache (seed-checked, see there). `Cells`
/// is the continent's cell provider, shared (`Arc<CellGenerator>`, as `ContinentNoise` holds it) or borrowed
/// (`&CellGenerator`); the shape's parameters are copied in.
pub struct RiverNetwork<Cells> {
    continent: Cells,
    shape: ShapeGenerator,
    lake_density: f32,
    /// `riverWarp`'s axes: `Source.builder().seed(X_OFFSET / Y_OFFSET).frequency(30).simplex()`.
    warp_x: Source,
    warp_y: Source,
    cache: ConcurrentLossyCache<RiverPieces>,
}

impl<Cells: ContinentCells> RiverNetwork<Cells> {
    /// `new RiverGenerator(continent, config)` without its `RiverCarver` (passed to [`Self::sample`]): the piece
    /// cache is `LossyCache.concurrent(RIVER_CACHE_SIZE, ...)`, `Environment.CORES` buckets.
    pub fn new(continent: Cells, shape: &ShapeGenerator, config: ContinentConfig) -> Self {
        Self::with_concurrency(continent, shape, config, available_processors())
    }

    /// [`Self::new`] with the piece cache's bucket count (`concurrency`, Java's `Environment.CORES`) given.
    pub fn with_concurrency(
        continent: Cells,
        shape: &ShapeGenerator,
        config: ContinentConfig,
        concurrency: i32,
    ) -> Self {
        let warp = |seed| {
            Source::new(
                SourceKind::Simplex,
                SourceConfig {
                    seed,
                    frequency: 30.0,
                    ..SourceConfig::default()
                },
            )
        };
        Self {
            continent,
            shape: *shape,
            cache: ConcurrentLossyCache::new(RIVER_CACHE_SIZE, concurrency),
            lake_density: config.rivers.lake_density,
            warp_x: warp(8_657_124),
            warp_y: warp(5_123_678),
        }
    }

    /// `computeNodes(seed, PosUtil.pack(cell_x, cell_y))`. An ocean cell has no pieces. A land cell scans its
    /// neighbours in [`DIRECTIONS`] order, keeping the lowest one at or below its own value (the last on ties) and
    /// taking the rivers of every other land neighbour whose strictly lowest neighbour it is (the inflows). Then it
    /// adds its own river to the lowest neighbour, if any, and a lake when it is a source (no inflow) and
    /// [`Self::has_lake`]. A source whose lowest neighbour's value is at most 0 has no pieces at all. Neighbour
    /// coordinates wrap like Java `int`s.
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

    /// `getNodes(seed, x, y)`: [`Self::compute_nodes`] through the lossy piece cache, keyed by `PosUtil.pack(x, y)`.
    /// The source's `LongCache` hits on the key alone and so returns pieces computed with another runtime seed; the
    /// port's cache hits only on the same key and seed, so this always equals `compute_nodes(seed, x, y)`.
    pub fn nodes(&self, seed: i32, cell_x: i32, cell_y: i32) -> Arc<RiverPieces> {
        self.cache
            .compute_if_absent(seed, pack(cell_x, cell_y), |seed, key| {
                self.compute_nodes(seed, unpack_left(key), unpack_right(key))
            })
    }

    /// `riverWarp.getX(seed, x, y)` and `getY`: `Domain.warp(x, y, constant(0.004))` in Noise2D 2cb4843, seed-first.
    /// `DomainWarp` maps each 0..1 simplex to -0.5..0.5 (`Map`: `-0.5f + (value - 0) / 1 * 1`, which is
    /// `-0.5 + value` bit for bit, unclamped) and scales it by `0.004f`; both axes read the unwarped point.
    pub fn warp(&self, seed: i32, x: f32, y: f32) -> (f32, f32) {
        (
            x + (-0.5 + self.warp_x.value_seed_first(seed, x, y)) * 0.004,
            y + (-0.5 + self.warp_y.value_seed_first(seed, x, y)) * 0.004,
        )
    }

    /// The newer source's public `RiverGenerator.sample(seed, x, y, NoiseSample)`: warps the point ([`Self::warp`]),
    /// samples the nearest nodes there ([`Self::sample_nodes`]) and carves `sample` with
    /// `carver` at the warped point. `terrains` is the world registry the carver sample's terrains come from.
    pub fn sample(
        &self,
        seed: i32,
        x: f32,
        y: f32,
        sample: &mut NoiseSample,
        carver: &RiverCarver,
        terrains: &TerrainRegistry,
    ) {
        let (point_x, point_y) = self.warp(seed, x, y);
        let mut nodes = CarverSample::new(terrains);
        self.sample_nodes(seed, point_x, point_y, &mut nodes);
        carver.carve(seed, point_x, point_y, sample, &nodes);
    }

    /// Not Java's (Horizon's `glacial_landforms`): [`Self::sample`], then [`RiverCarver::glacial_trough`] over the same
    /// nodes, with `glaciation` asked for the height the sample had before the carve.
    #[allow(clippy::too_many_arguments)]
    pub fn sample_glacial(
        &self,
        seed: i32,
        x: f32,
        y: f32,
        sample: &mut NoiseSample,
        carver: &RiverCarver,
        terrains: &TerrainRegistry,
        glaciation: impl FnOnce(f32) -> f32,
    ) {
        let (point_x, point_y) = self.warp(seed, x, y);
        let mut nodes = CarverSample::new(terrains);
        self.sample_nodes(seed, point_x, point_y, &mut nodes);
        let before = (
            sample.height_noise,
            sample.base_noise,
            sample.continent_noise,
        );
        carver.carve(seed, point_x, point_y, sample, &nodes);
        carver.glacial_trough(before, sample, &nodes, glaciation);
    }

    /// The newer source's `RiverGenerator.sample(seed, x, y, NoiseSample)` between the warp and the carver call:
    /// `localRiverSample.get().reset()`, then the private `sample(seed, x, y, CarverSample)`. It takes the
    /// nearest cell of the unadjusted point, adjusts the point for the cell shape, and runs [`sample_node`] over the
    /// river nodes and the lake nodes of the 3x3 cells around that centre (rows outer, each in insertion order).
    /// The winners are recorded by [`Self::record_node`]; a sample with no winner is invalidated (distance NaN).
    /// The window follows Java's `int` loop bounds: empty when `centre ± 1` wraps, never-ending (a panic here)
    /// when the upper bound is `Integer.MAX_VALUE`. [`CellGenerator`]'s centres stay far from both, so neither
    /// happens with it. Pieces come from [`Self::nodes`] (the piece cache), which equals computing them directly.
    ///
    /// [`CellGenerator`]: super::terraforged_continent_cells::CellGenerator
    pub fn sample_nodes(&self, seed: i32, x: f32, y: f32, sample: &mut CarverSample) {
        sample.reset();
        let (center_x, center_y) = self.continent.nearest_cell(seed, x, y);
        // Java adjusts the point after the nearest-cell lookup (which adjusts its own copy).
        let x = self.continent.cell_shape().adjust_x(x);
        let y = self.continent.cell_shape().adjust_y(y);
        let mut river = None;
        let mut lake = None;
        let Some(rows) = window(center_y.wrapping_sub(1)) else {
            panic!(
                "TerraForged's RiverGenerator.sample loops forever around cell ({center_x}, {center_y})"
            );
        };
        let columns = window(center_x.wrapping_sub(1));
        for cell_y in rows {
            let Some(columns) = columns.clone() else {
                panic!(
                    "TerraForged's RiverGenerator.sample loops forever around cell ({center_x}, {center_y})"
                );
            };
            for cell_x in columns {
                let pieces = self.nodes(seed, cell_x, cell_y);
                for node in &pieces.rivers {
                    sample_node(x, y, node, &mut river, &mut sample.river);
                }
                for node in &pieces.lakes {
                    sample_node(x, y, node, &mut lake, &mut sample.lake);
                }
            }
        }
        self.record_node(river.as_ref(), &mut sample.river);
        self.record_node(lake.as_ref(), &mut sample.lake);
    }

    /// `recordNode`: for a winner, the distance becomes its square root (`NoiseUtil.sqrt`, `(float)
    /// Math.sqrt`), `position` the node's radius and `level` the shape's base noise of its height
    /// ([`Self::base_noise`]), both at the sample's projection; without one the sample is invalidated (its other fields keep their reset values).
    fn record_node(&self, node: Option<&RiverNode>, sample: &mut NodeSample) {
        if let Some(node) = node {
            let level = node.height(sample.projection);
            let radius = node.radius(sample.projection);
            sample.distance = (sample.distance as f64).sqrt() as f32;
            sample.position = radius;
            sample.level = self.base_noise(level);
        } else {
            sample.invalidate();
        }
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
            || self.base_noise(cell.noise()) < 0.25
    }

    /// `continent.shapeGenerator.getBaseNoise(value)` as the newer source runs it: through Noise2D 2cb4843's
    /// `NoiseUtil.map` ([`map_current`]), which gives 1 where the base falloffs are equal or infinitely apart and
    /// the jar's (`ShapeGenerator::base_noise`) gives NaN. (`hasLake`'s `< 0.25` is false for both.)
    fn base_noise(&self, value: f32) -> f32 {
        let (min, max) = (self.shape.base_falloff_min, self.shape.base_falloff_max);
        map_current(value, min, max, max - min)
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

/// `sampleNode`: `node` wins when its squared distance at its projection is strictly below the sample's (so the
/// first of equal distances stays, and NaN never wins); the sample then takes that distance and projection.
fn sample_node(
    x: f32,
    y: f32,
    node: &RiverNode,
    nearest: &mut Option<RiverNode>,
    sample: &mut NodeSample,
) {
    let projection = node.projection(x, y);
    let distance = node.distance_squared(x, y, projection);
    if distance < sample.distance {
        *nearest = Some(*node);
        sample.distance = distance;
        sample.projection = projection;
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
        terraforged_engine::{ControlPoints, TerrainRegistry, WorldControlPoints},
        terraforged_terrain_levels::NoiseLevels,
    };
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// The oracles' `Environment.CORES` stub: the caches' bucket count, so Rust's cache layout is Java's.
    const CORES: i32 = 6;

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

        fn config(&self) -> ContinentConfig {
            let mut config = ContinentConfig::default();
            config.shape.cell_shape = self.shape;
            config.shape.cell_source = self.source;
            (config.shape.seed0, config.shape.seed1) = self.seeds;
            config.shape.jitter = self.jitter;
            config.shape.threshold = self.threshold;
            config.rivers.lake_density = self.lake_density;
            config.rivers.seed = 0x5eed; // the newer source's network never reads it
            config
        }

        /// Folds `computeNodes` at every cell of `xs` x `ys` (y outer, as the oracle).
        fn fold(&self, fold: &mut Fold, seed: i32, xs: &[i32], ys: &[i32]) {
            let config = self.config();
            let cells = CellGenerator::with_concurrency(config, CORES);
            let shape =
                ShapeGenerator::new(config, ControlPoints::new(WorldControlPoints::default()));
            let network = RiverNetwork::with_concurrency(&cells, &shape, config, CORES);
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

    /// `RiverSamplingOracle`'s fold: river then lake, each sample's projection, distance, position and level
    /// (canonical NaN bits); `rivers` counts the invalid samples.
    fn fold_sample(fold: &mut Fold, sample: &NodeSample) {
        fold.rivers += usize::from(sample.is_invalid());
        for value in [
            sample.projection,
            sample.distance,
            sample.position,
            sample.level,
        ] {
            fold.float(value);
        }
    }

    /// Samples every point of `points` on a new network (as the oracle's `point`), folding both samples.
    fn fold_points(
        fold: &mut Fold,
        setup: &Setup,
        seed: i32,
        points: impl IntoIterator<Item = (f32, f32)>,
    ) {
        let config = setup.config();
        let cells = CellGenerator::with_concurrency(config, CORES);
        let shape = ShapeGenerator::new(config, ControlPoints::new(WorldControlPoints::default()));
        let network = RiverNetwork::with_concurrency(&cells, &shape, config, CORES);
        fold_network(fold, &network, seed, points);
    }

    /// [`fold_points`] on a given network (the oracle's `shared` lines reuse one across runtime seeds).
    fn fold_network<Cells: ContinentCells>(
        fold: &mut Fold,
        network: &RiverNetwork<Cells>,
        seed: i32,
        points: impl IntoIterator<Item = (f32, f32)>,
    ) {
        let mut sample = CarverSample::new(&TerrainRegistry::new());
        for (x, y) in points {
            network.sample_nodes(seed, x, y, &mut sample);
            fold_sample(fold, &sample.river);
            fold_sample(fold, &sample.lake);
        }
    }

    /// Row-major `xs` x `ys` points, y outer.
    fn points(xs: &[f32], ys: &[f32]) -> Vec<(f32, f32)> {
        ys.iter()
            .flat_map(|&y| xs.iter().map(move |&x| (x, y)))
            .collect()
    }

    fn steps(radius: i32, step: f32, offset: f32) -> Vec<f32> {
        (-radius..=radius)
            .map(|i| i as f32 * step + offset)
            .collect()
    }

    /// `(invalid samples, hash)` per line of the oracle's output, configurations in `SHAPES` x `SOURCES` order.
    fn check_sample(fold: &mut Fold, name: &str, expected: (usize, u64)) {
        assert_eq!((fold.rivers, fold.hash), expected, "{name}");
        *fold = Fold::new();
    }

    const MIN: f32 = f32::from_bits(1);
    #[rustfmt::skip]
    const POINTS: [f32; 26] = [f32::NAN, f32::NEG_INFINITY, f32::INFINITY, -0.0, 0.0, MIN, -MIN, 1e30, -1e30,
        f32::MAX, -f32::MAX, 2_147_483_520.0, 2_147_483_648.0, -2_147_483_648.0, -2_147_483_520.0, 16_777_216.0,
        -16_777_216.0, 16_777_218.0, 0.5, -0.5, 1.75, -2.25, 123.4, -98.76, 0.999_999_94, -1.0];

    /// One pipeline's stages: network, carver, shape and cells.
    type Stages<'a, 'b> = (
        &'a RiverNetwork<&'b CellGenerator>,
        &'a RiverCarver,
        &'a ShapeGenerator,
        &'a CellGenerator,
    );

    /// The river pipeline of one configuration (as `RiverPipelineOracle`'s `pipe`).
    struct Pipeline {
        config: ContinentConfig,
        levels: NoiseLevels,
        terrains: TerrainRegistry,
    }

    impl Pipeline {
        fn new(setup: &Setup, river_seed: i32, levels: NoiseLevels) -> Self {
            let mut config = ContinentConfig::default();
            config.shape.cell_shape = setup.shape;
            config.shape.cell_source = setup.source;
            (config.shape.seed0, config.shape.seed1) = setup.seeds;
            config.rivers.seed = river_seed;
            Self {
                config,
                levels,
                terrains: TerrainRegistry::new(),
            }
        }

        /// The oracle's `fold`: the four noises and the terrain code (0 none, 1 river, 2 lake, 3 other); `rivers`
        /// counts the carved samples.
        fn fold(&self, fold: &mut Fold, sample: &NoiseSample) {
            for value in [
                sample.continent_noise,
                sample.base_noise,
                sample.height_noise,
                sample.river_noise,
            ] {
                fold.float(value);
            }
            let terrain = &sample.terrain_type;
            let code = if terrain.is_same(&self.terrains.get_by_id(5)) {
                1
            } else if terrain.is_same(&self.terrains.get_by_id(6)) {
                2
            } else if terrain.is_same(&self.terrains.get_by_id(0)) {
                0
            } else {
                3
            };
            fold.rivers += usize::from(code == 1 || code == 2);
            fold.add(code);
        }

        /// Runs `f(network, carver, shape, cells)` on this pipeline.
        fn run(
            &self,
            f: impl FnOnce(&RiverNetwork<&CellGenerator>, &RiverCarver, &ShapeGenerator, &CellGenerator),
        ) {
            let cells = CellGenerator::with_concurrency(self.config, CORES);
            let shape = ShapeGenerator::new(
                self.config,
                ControlPoints::new(WorldControlPoints::default()),
            );
            let network = RiverNetwork::with_concurrency(&cells, &shape, self.config, CORES);
            f(
                &network,
                &RiverCarver::new(self.levels, self.config),
                &shape,
                &cells,
            );
        }

        /// The oracle's `shaped` at each point (folding the warped point first when `warp`): the shape's sample with
        /// the stand-in height `toHeightNoise(base, 0.35)`, then the pipeline.
        fn shaped(&self, fold: &mut Fold, seed: i32, points: &[(f32, f32)], warp: bool) {
            self.run(|network, carver, shape, cells| {
                self.shaped_on(fold, (network, carver, shape, cells), seed, points, warp);
            });
        }

        /// [`Self::shaped`] on a given pipeline (the oracle's `shared` line reuses one across runtime seeds).
        fn shaped_on(
            &self,
            fold: &mut Fold,
            (network, carver, shape, cells): Stages,
            seed: i32,
            points: &[(f32, f32)],
            warp: bool,
        ) {
            for &(x, y) in points {
                if warp {
                    let (warp_x, warp_y) = network.warp(seed, x, y);
                    fold.float(warp_x);
                    fold.float(warp_y);
                }
                let mut sample = NoiseSample::new(&self.terrains);
                shape.sample(cells, seed, x, y, &mut sample);
                sample.height_noise = self.levels.to_height_noise(sample.base_noise, 0.35);
                network.sample(seed, x, y, &mut sample, carver, &self.terrains);
                self.fold(fold, &sample);
            }
        }

        /// The oracle's `synthetic` line (hand-set samples), at runtime seed 42.
        fn synthetic(&self, fold: &mut Fold) {
            self.run(|network, carver, _, _| {
                for (x, y) in points(&steps(3, 0.5, 0.2), &steps(3, 0.5, -0.1)) {
                    let mut sample = NoiseSample::new(&self.terrains);
                    for continent in [0.25, 0.5, 0.75, 1.0, f32::NAN] {
                        for base in [0.0, 0.3, 0.6, 1.0, -1.0] {
                            for (height, river) in [0.2, 0.5, 0.9]
                                .into_iter()
                                .flat_map(|h| [(h, 1.0), (h, 0.5)])
                            {
                                sample.continent_noise = continent;
                                sample.base_noise = base;
                                sample.height_noise = height;
                                sample.river_noise = river;
                                sample.terrain_type = self.terrains.get_by_id(0);
                                network.sample(42, x, y, &mut sample, carver, &self.terrains);
                                self.fold(fold, &sample);
                            }
                        }
                    }
                }
            });
        }

        /// The oracle's `edges` (folding the warped points) and `falloffs` lines: one hand-set sample per point.
        fn hand_set(&self, fold: &mut Fold, points: &[(f32, f32)], warp: bool) {
            self.run(|network, carver, _, _| {
                for &(x, y) in points {
                    if warp {
                        let (warp_x, warp_y) = network.warp(42, x, y);
                        fold.float(warp_x);
                        fold.float(warp_y);
                    }
                    let mut sample = NoiseSample::new(&self.terrains);
                    sample.continent_noise = 0.75;
                    sample.base_noise = 0.6;
                    sample.height_noise = 0.5;
                    network.sample(42, x, y, &mut sample, carver, &self.terrains);
                    self.fold(fold, &sample);
                }
            });
        }
    }

    /// [`CellGenerator`] without its cache: every cell and nearest cell computed directly.
    struct Uncached<'a>(&'a CellGenerator);

    impl ContinentCells for Uncached<'_> {
        fn cell(&self, seed: i32, x: i32, y: i32) -> CellPoint {
            self.0.compute_cell(seed, x, y)
        }

        fn nearest_cell(&self, seed: i32, x: f32, y: f32) -> (i32, i32) {
            self.0.nearest_cell_with(x, y, |cell_x, cell_y| {
                self.0.compute_cell(seed, cell_x, cell_y)
            })
        }

        fn cell_shape(&self) -> CellShape {
            self.0.cell_shape()
        }
    }

    /// [`CellGenerator`] with its cache, counting the cell lookups (the nearest-cell search's included).
    struct Counting<'a>(&'a CellGenerator, AtomicUsize);

    impl ContinentCells for Counting<'_> {
        fn cell(&self, seed: i32, x: i32, y: i32) -> CellPoint {
            self.1.fetch_add(1, Ordering::Relaxed);
            self.0.cell(seed, x, y)
        }

        fn nearest_cell(&self, seed: i32, x: f32, y: f32) -> (i32, i32) {
            self.0
                .nearest_cell_with(x, y, |cell_x, cell_y| self.cell(seed, cell_x, cell_y))
        }

        fn cell_shape(&self) -> CellShape {
            self.0.cell_shape()
        }
    }

    fn pieces_hash(pieces: &RiverPieces) -> (usize, usize, u64) {
        let mut fold = Fold::new();
        fold.cell(pieces);
        (fold.rivers, fold.lakes, fold.hash)
    }

    fn samples_hash(sample: &CarverSample) -> (usize, u64) {
        let mut fold = Fold::new();
        fold_sample(&mut fold, &sample.river);
        fold_sample(&mut fold, &sample.lake);
        (fold.rivers, fold.hash)
    }

    /// The nearest-node sampling with no cache at all: a new piece cache per sample over uncached cells.
    fn direct_sample(
        config: ContinentConfig,
        shape: &ShapeGenerator,
        cells: &CellGenerator,
        seed: i32,
        x: f32,
        y: f32,
    ) -> CarverSample {
        let uncached = Uncached(cells);
        let network = RiverNetwork::with_concurrency(&uncached, shape, config, CORES);
        let mut sample = CarverSample::new(&TerrainRegistry::new());
        network.sample_nodes(seed, x, y, &mut sample);
        sample
    }

    /// Both caches never change a result: one network shared by 8 threads, each walking every runtime seed over
    /// the same 7x7 cells and 4x4 points in its own order (so slots keep changing seed and key), gives the pieces
    /// and samples of the direct computation.
    #[test]
    fn cached_pieces_and_samples_equal_direct_for_many_seeds_and_threads() {
        let mut seeds = vec![
            0,
            1,
            -1,
            42,
            i32::MAX,
            i32::MIN,
            -827_614,
            i32::MAX - 20_106,
        ];
        seeds.extend((0..8).map(|i: i32| i.wrapping_mul(0x9e37_79b9_u32 as i32)));
        for (shape, source) in [
            (CellShape::Square, CellSource::Perlin),
            (CellShape::Hexagon, CellSource::Cubic),
        ] {
            let setup = Setup::new(shape, source);
            let config = setup.config();
            let cells = CellGenerator::with_concurrency(config, CORES);
            let shape =
                ShapeGenerator::new(config, ControlPoints::new(WorldControlPoints::default()));
            let network = RiverNetwork::with_concurrency(&cells, &shape, config, CORES);
            let uncached = Uncached(&cells);
            let direct = RiverNetwork::with_concurrency(&uncached, &shape, config, CORES);
            std::thread::scope(|scope| {
                for thread in 0..8 {
                    let (network, direct, seeds, cells, shape) =
                        (&network, &direct, &seeds, &cells, &shape);
                    scope.spawn(move || {
                        let mut sample = CarverSample::new(&TerrainRegistry::new());
                        for round in 0..seeds.len() {
                            let seed = seeds[(round * 3 + thread * 5) % seeds.len()];
                            for y in -3..=3 {
                                for x in -3..=3 {
                                    let (x, y) = if thread % 2 == 0 { (x, y) } else { (-y, x) };
                                    assert_eq!(
                                        pieces_hash(&network.nodes(seed, x, y)),
                                        pieces_hash(&direct.compute_nodes(seed, x, y)),
                                        "{seed} ({x}, {y})"
                                    );
                                }
                            }
                            for (x, y) in points(&steps(2, 0.6, 0.1), &steps(2, 0.6, -0.3))
                                .into_iter()
                                .take(16)
                            {
                                network.sample_nodes(seed, x, y, &mut sample);
                                assert_eq!(
                                    samples_hash(&sample),
                                    samples_hash(&direct_sample(config, shape, cells, seed, x, y)),
                                    "{seed} ({x}, {y})"
                                );
                            }
                        }
                    });
                }
            });
        }
    }

    /// What the caches save per sample: without them, `sample_nodes` reads the nearest-cell search's 9 cells, then
    /// computes the pieces of the 9 cells around it (a new piece list each, reading the cell and its neighbours, and
    /// theirs for a land cell). With them, a sample whose neighbourhood is cached reads only the 9 cells of
    /// the nearest-cell search (cache hits) and computes no pieces (every `computeNodes` reads at least one cell).
    #[test]
    fn cached_sampling_reads_only_the_nearest_cells() {
        let config = Setup::new(CellShape::Square, CellSource::Perlin).config();
        let cells = CellGenerator::with_concurrency(config, CORES);
        let shape = ShapeGenerator::new(config, ControlPoints::new(WorldControlPoints::default()));
        let grid = points(&steps(4, 0.25, 0.0625), &steps(4, 0.25, -0.125));
        let samples = grid.len();
        let mut sample = CarverSample::new(&TerrainRegistry::new());
        // Direct: a new piece cache per sample.
        let counting = Counting(&cells, AtomicUsize::new(0));
        for &(x, y) in &grid {
            RiverNetwork::with_concurrency(&counting, &shape, config, CORES).sample_nodes(
                42,
                x,
                y,
                &mut sample,
            );
        }
        let direct = counting.1.load(Ordering::Relaxed);
        // Cached: one network, the grid twice; the second pass hits the piece cache everywhere.
        let counting = Counting(&cells, AtomicUsize::new(0));
        let network = RiverNetwork::with_concurrency(&counting, &shape, config, CORES);
        for &(x, y) in &grid {
            network.sample_nodes(42, x, y, &mut sample);
        }
        let cold = counting.1.swap(0, Ordering::Relaxed);
        for &(x, y) in &grid {
            network.sample_nodes(42, x, y, &mut sample);
        }
        let warm = counting.1.load(Ordering::Relaxed);
        // 81 samples, about 72 cell reads each without caches (ocean cells stop after one read).
        assert_eq!((direct, cold, warm), (5810, 895, 9 * samples));
    }

    /// TerraForged shares one `RiverGenerator` (warp, carver) across its worker threads.
    #[test]
    fn river_pipeline_is_send_and_sync() {
        fn shared<T: Send + Sync>() {}
        shared::<RiverNetwork<CellGenerator>>();
        shared::<RiverCarver>();
    }

    /// `RiverPipelineOracle.java` (`.local/terraforged/oracles/river_pipeline/run.sh`): the newer source's public
    /// `RiverGenerator.sample(seed, x, y, NoiseSample)` end to end (warp, nearest nodes, seed-first erosion carve),
    /// caches bypassed, per configuration in `SHAPES` x `SOURCES` order: a 25x25 grid over the shape's sample at two
    /// noise levels, hand-set samples, edge points (with the warped points), runtime and shape seeds (with the
    /// warped points), degenerate base falloffs and `rivers.seed`s around the `+ 21221` wrap. The jar (seed
    /// mapping) prints the same lines except `rivers` (its erosion seed ignores the runtime seed) and `falloffs`
    /// (its `NoiseUtil.map` gives NaN for a zero or infinite range). `(carved samples, hash)` per line.
    #[test]
    fn river_pipeline_matches_current_java() {
        #[rustfmt::skip]
        const EXPECTED: [[(usize, u64); 7]; 6] = [
            [(41, 0x78fafcf1c28d9a79), (45, 0xbf381479070b76a8), (600, 0xaac9f8fe8b277d87), (36, 0x5a51039d01f3e7ce), (42, 0x6e0d75ae6b904782), (165, 0xb4bbcb1ba97a18c0), (48, 0x33cb76f39c6aee22)],
            [(24, 0xd409cbcbc5eb854e), (28, 0xe95e8ad21ba1dfaf), (600, 0xbc27e9de9b3e5f71), (45, 0xd41fab625b366238), (40, 0x93f215b46af66b37), (97, 0x998b28bd09b2bf36), (36, 0x5c65405b514d42fc)],
            [(61, 0x3cee7166f9cf5f52), (70, 0xa9036b15fff0faa6), (600, 0x67eddf12cc0d3f4b), (47, 0xb068d2e0c34cd6c9), (37, 0x9c24212ce3e8482b), (244, 0xe201abd8b5225f2a), (78, 0x19325249466e6d04)],
            [(31, 0x50d043d78592d68d), (40, 0x7325e41767027060), (600, 0x429fe2452fe4bba1), (39, 0x7d27a2bc9b369d6c), (29, 0x6c6e7c2b8bba9ea8), (125, 0x1c45a0cc95924e1d), (66, 0x971efeeac500edfc)],
            [(19, 0xc914583a0772235f), (26, 0x26e6f13fd803007d), (450, 0xe0538f8807e2f67f), (36, 0x7f4cc77330799ef1), (30, 0x01727813f9e257d5), (77, 0x59d88b08e8fde027), (84, 0xe89dfa314049b210)],
            [(50, 0xbbfb5270640c6882), (54, 0xce28ffe950ae9dbf), (450, 0xd2d327820489eeab), (53, 0xc78b9162e5f3bb50), (30, 0x31509070fcea72b1), (200, 0xfb53a3e983dacf51), (78, 0x8b9e10bdb44b83c9)],
        ];
        const NAMES: [&str; 7] = [
            "grid0",
            "grid1",
            "synthetic",
            "edges",
            "seeds",
            "falloffs",
            "rivers",
        ];
        const INF: f32 = f32::INFINITY;
        // Equal and infinite base falloffs: `recordNode`'s `getBaseNoise` over a zero or infinite range (1 in the
        // newer source, NaN in the jar).
        const FALLOFFS: [(f32, f32); 4] = [(0.1, 0.1), (0.0, 0.0), (-0.3, INF), (-INF, INF)];
        #[rustfmt::skip]
        const RUNTIME: [i32; 11] = [0, 42, -1, i32::MAX, i32::MIN, -827_614, i32::MAX - 20_106, i32::MIN + 9_802,
            123_456_789, -8_657_124, i32::MAX - 5_123_677];
        const RIVER_SEEDS: [i32; 6] = [0, 0x5eed, i32::MAX, i32::MIN, -21_221, i32::MAX - 21_220];
        const BASE: [(i32, i32); 3] = [(0, 0), (i32::MAX, i32::MIN), (-6569, 987_654_321)];
        let levels = [
            NoiseLevels::new(false, 1.0, 62, 22, 256, 128),
            NoiseLevels::new(true, 2.5, 62, 22, 480, 128),
        ];
        let grid = points(&steps(12, 0.25, 0.0625), &steps(12, 0.25, -0.125));
        let small = points(&steps(2, 0.75, 0.3), &steps(2, 0.75, -0.2));
        for (index, (shape, source)) in SHAPES
            .into_iter()
            .flat_map(|shape| SOURCES.map(|source| (shape, source)))
            .enumerate()
        {
            let mut setup = Setup::new(shape, source);
            let mut lines = Vec::new();
            let mut fold = Fold::new();
            let mut line = |fold: &mut Fold| {
                lines.push((fold.rivers, fold.hash));
                *fold = Fold::new();
            };
            for levels in levels {
                Pipeline::new(&setup, 0, levels).shaped(&mut fold, 42, &grid, false);
                line(&mut fold);
            }
            let pipeline = Pipeline::new(&setup, 0, levels[0]);
            pipeline.synthetic(&mut fold);
            line(&mut fold);
            pipeline.hand_set(&mut fold, &points(&POINTS, &POINTS), true);
            line(&mut fold);
            for seed in RUNTIME {
                for base in BASE {
                    setup.seeds = base;
                    Pipeline::new(&setup, 0, levels[0]).shaped(&mut fold, seed, &small, true);
                }
            }
            let seeds = (fold.rivers, fold.hash);
            line(&mut fold);
            // The oracle's `shared` line: one pipeline per shape-seed pair across all runtime seeds. The port's
            // caches hit only on the same seed, so it equals `seeds` (the source's cached run doesn't).
            let pipelines = BASE.map(|base| {
                setup.seeds = base;
                let pipeline = Pipeline::new(&setup, 0, levels[0]);
                let cells = CellGenerator::with_concurrency(pipeline.config, CORES);
                let shape = ShapeGenerator::new(
                    pipeline.config,
                    ControlPoints::new(WorldControlPoints::default()),
                );
                let carver = RiverCarver::new(pipeline.levels, pipeline.config);
                (pipeline, cells, shape, carver)
            });
            let networks = pipelines.each_ref().map(|(p, cells, shape, _)| {
                RiverNetwork::with_concurrency(cells, shape, p.config, CORES)
            });
            for seed in RUNTIME {
                for ((pipeline, cells, shape, carver), network) in pipelines.iter().zip(&networks) {
                    pipeline.shaped_on(
                        &mut fold,
                        (network, carver, shape, cells),
                        seed,
                        &small,
                        true,
                    );
                }
            }
            assert_eq!((fold.rivers, fold.hash), seeds, "shared {index}");
            fold = Fold::new();
            setup.seeds = (0, 0);
            for (min, max) in FALLOFFS {
                let mut pipeline = Pipeline::new(&setup, 0, levels[0]);
                pipeline.config.shape.base_falloff_min = min;
                pipeline.config.shape.base_falloff_max = max;
                pipeline.hand_set(&mut fold, &grid, false);
            }
            line(&mut fold);
            for river_seed in RIVER_SEEDS {
                for seed in RUNTIME {
                    Pipeline::new(&setup, river_seed, levels[0])
                        .shaped(&mut fold, seed, &small, false);
                }
            }
            line(&mut fold);
            for (line, (name, expected)) in lines
                .into_iter()
                .zip(NAMES.into_iter().zip(EXPECTED[index]))
            {
                assert_eq!(line, expected, "{name} {index}");
            }
        }
    }

    /// `RiverSamplingOracle.java` (`.local/terraforged/oracles/river_network/run-sampling.sh`): the private
    /// `RiverGenerator.sample(.., CarverSample)` of the jar and of the newer source (equal under the seed mapping)
    /// on a quarter-cell grid, over runtime and shape seeds, and at edge points (NaN, infinities, signed zeros,
    /// subnormals, float extremes, the `int` boundaries; huge points take the jar's (0, 0) centre).
    #[test]
    fn node_sampling_matches_java() {
        let grid = [
            (706, 0x2c17faba74c46bb4),
            (472, 0x79cde16049b3f6cd),
            (139, 0xb066fde93ea82d0b),
            (648, 0x7e2bb16f1f76f6be),
            (471, 0x81b315f07a01beda),
            (193, 0x107cea23980bcffd),
        ];
        let seeds = [
            (751, 0xe4bf7b92cd459ba2),
            (521, 0x525211a77578d080),
            (739, 0xdfb1974f2c3ec05e),
            (747, 0x63db1def59f9ca3e),
            (545, 0xbd195ad50c627c07),
            (731, 0x09363b705b05e264),
        ];
        let edges = [
            (1071, 0x9b941d3645ffd2fe),
            (1009, 0xc47f9143e7a14885),
            (876, 0x308f816a061548fc),
            (1032, 0x7256fc16be3eaf24),
            (984, 0x156ad26d67201aa2),
            (809, 0xf50443cd322f9ec2),
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
        let mut fold = Fold::new();
        for (index, (shape, source)) in SHAPES
            .into_iter()
            .flat_map(|shape| SOURCES.map(|source| (shape, source)))
            .enumerate()
        {
            let mut setup = Setup::new(shape, source);
            let grid_points = points(&steps(12, 0.25, 0.0625), &steps(12, 0.25, -0.125));
            fold_points(&mut fold, &setup, 42, grid_points);
            check_sample(&mut fold, "grid", grid[index]);
            let seed_points = points(&steps(2, 0.75, 0.3), &steps(2, 0.75, -0.2));
            for seed in RUNTIME {
                for base in BASE {
                    setup.seeds = base;
                    fold_points(&mut fold, &setup, seed, seed_points.iter().copied());
                }
            }
            check_sample(&mut fold, "seeds", seeds[index]);
            // The oracle's `shared` lines: one network per shape-seed pair across all runtime seeds equals `seeds`
            // (the source's cached run doesn't: its caches hit on the cell alone).
            let configs = BASE.map(|base| {
                setup.seeds = base;
                setup.config()
            });
            let cells = configs.map(|config| CellGenerator::with_concurrency(config, CORES));
            let shapes = configs.map(|config| {
                ShapeGenerator::new(config, ControlPoints::new(WorldControlPoints::default()))
            });
            let networks: Vec<_> = (0..BASE.len())
                .map(|i| RiverNetwork::with_concurrency(&cells[i], &shapes[i], configs[i], CORES))
                .collect();
            for seed in RUNTIME {
                for network in &networks {
                    fold_network(&mut fold, network, seed, seed_points.iter().copied());
                }
            }
            check_sample(&mut fold, "shared", seeds[index]);
            setup.seeds = (0, 0);
            fold_points(&mut fold, &setup, 42, points(&POINTS, &POINTS));
            check_sample(&mut fold, "edges", edges[index]);
        }
    }

    #[rustfmt::skip]
    fn node(ax: f32, ay: f32, bx: f32, by: f32, ah: f32, bh: f32, ar: f32, br: f32, displacement: f32) -> RiverNode {
        RiverNode { ax, ay, bx, by, ah, bh, ar, br, displacement }
    }

    /// The oracle's `nodes` line: `sampleNode` over hand-made lists (ties between equal nodes and mirrored ones, a
    /// zero-length node, NaN and infinite fields, an empty list) at edge points, then `recordNode`; folds the
    /// winner's index (-1 for none) and the sample.
    #[test]
    fn nearest_node_choice_matches_java() {
        const NAN: f32 = f32::NAN;
        const INF: f32 = f32::INFINITY;
        let a = node(0.0, 0.0, 1.0, 0.0, 0.2, 0.3, 0.5, 1.0, 0.0);
        #[rustfmt::skip]
        let lists: [Vec<RiverNode>; 11] = [
            vec![],
            vec![a],
            vec![a, node(0.0, 0.0, 1.0, 0.0, 0.9, 0.1, 0.25, 0.75, 0.0)],
            vec![node(0.0, 1.0, 1.0, 1.0, 0.4, 0.6, 0.1, 0.2, 0.0), node(0.0, -1.0, 1.0, -1.0, 0.7, 0.8, 0.3, 0.4, 0.0)],
            vec![node(0.5, 0.5, 0.5, 0.5, 0.6, 0.6, 1.0, 1.0, 0.0), a],
            vec![node(NAN, 0.0, 1.0, 0.0, 0.5, 0.5, 1.0, 1.0, 0.0), node(0.0, NAN, 1.0, 0.0, 0.5, 0.5, 1.0, 1.0, 0.0)],
            vec![node(NAN, 0.0, 1.0, 0.0, 0.5, 0.5, 1.0, 1.0, 0.0), a, node(0.0, 0.0, 1.0, 0.0, 0.1, 0.1, 0.1, 0.1, 0.5)],
            vec![node(0.0, 0.0, 1.0, 0.0, NAN, 0.5, NAN, 1.0, 0.0), node(0.0, 0.0, INF, 0.0, 0.5, 0.5, 1.0, 1.0, 0.0)],
            vec![node(-INF, 0.0, INF, 0.0, 0.5, 0.5, 1.0, 1.0, 0.0), node(0.0, 0.0, 1.0, 0.0, 0.5, 0.5, 1.0, 1.0, NAN)],
            vec![node(0.0, 0.0, 1.0, 0.0, 0.5, 0.5, 1.0, 1.0, 1e30), node(0.0, 0.0, 1.0, 0.0, 0.5, 0.5, 1.0, 1.0, -0.0), a],
            vec![node(1e30, 1e30, -1e30, -1e30, 0.3, 0.7, 0.2, 0.9, 0.1), node(MIN, -MIN, 0.0, 0.0, 0.5, 0.5, 1.0, 1.0, 0.0)],
        ];
        #[rustfmt::skip]
        const NODE_POINTS: [f32; 15] = [0.0, -0.0, 0.5, 0.25, -0.25, 1.0, 1.5, -1.0, 1e20, -1e20, MIN, f32::MAX, INF,
            -INF, NAN];
        let setup = Setup::new(CellShape::Square, CellSource::Perlin);
        let config = setup.config();
        let cells = CellGenerator::new(config);
        let shape = ShapeGenerator::new(config, ControlPoints::new(WorldControlPoints::default()));
        let network = RiverNetwork::new(&cells, &shape, config);
        let terrains = TerrainRegistry::new();
        let mut sample = CarverSample::new(&terrains).river;
        let mut fold = Fold::new();
        for list in &lists {
            for &y in &NODE_POINTS {
                for &x in &NODE_POINTS {
                    sample.reset();
                    let mut nearest = None;
                    let mut winner = -1;
                    for (index, node) in list.iter().enumerate() {
                        let distance = sample.distance.to_bits();
                        sample_node(x, y, node, &mut nearest, &mut sample);
                        // A win (strictly lower) always changes the distance's bits.
                        if sample.distance.to_bits() != distance {
                            winner = index as i32;
                        }
                    }
                    fold.add(winner as u32 as u64);
                    network.record_node(nearest.as_ref(), &mut sample);
                    fold_sample(&mut fold, &sample);
                }
            }
        }
        check_sample(&mut fold, "nodes", (1866, 0x5a39aac3cad2ec2a));
    }

    /// A stub continent whose nearest cell is fixed (counting `cell` calls), for Java's `int` loop bounds around
    /// the centre. Its cells are all default points (ocean, so no pieces).
    struct Centre(i32, i32, std::cell::Cell<usize>);

    impl ContinentCells for Centre {
        fn cell(&self, _seed: i32, _x: i32, _y: i32) -> CellPoint {
            self.2.set(self.2.get() + 1);
            CellPoint::default()
        }

        fn nearest_cell(&self, _seed: i32, _x: f32, _y: f32) -> (i32, i32) {
            (self.0, self.1)
        }

        fn cell_shape(&self) -> CellShape {
            CellShape::Square
        }
    }

    /// Cells visited around a stub centre: 9, or none where `centre ± 1` wraps (Java's loops are then empty).
    fn visited(centre_x: i32, centre_y: i32) -> usize {
        let config = ContinentConfig::default();
        let cells = Centre(centre_x, centre_y, std::cell::Cell::new(0));
        let shape = ShapeGenerator::new(config, ControlPoints::new(WorldControlPoints::default()));
        let network = RiverNetwork::new(&cells, &shape, config);
        let mut sample = CarverSample::new(&TerrainRegistry::new());
        network.sample_nodes(0, 0.0, 0.0, &mut sample);
        assert!(sample.river.is_invalid() && sample.lake.is_invalid());
        cells.2.get()
    }

    #[test]
    fn sampling_window_follows_java_int_loops() {
        for centre in [0, 1, -1, i32::MAX - 2, i32::MIN + 1, 123_456] {
            assert_eq!(visited(centre, centre), 9, "{centre}");
        }
        // Upper bound wraps below the lower one: empty rows, or empty columns in every row.
        for (x, y) in [
            (0, i32::MAX),
            (0, i32::MIN),
            (i32::MAX, 0),
            (i32::MIN, 0),
            (i32::MAX - 1, i32::MAX),
        ] {
            assert_eq!(visited(x, y), 0, "({x}, {y})");
        }
    }

    #[test]
    #[should_panic(expected = "loops forever")]
    fn sampling_rows_ending_at_int_max_loop_forever_in_java() {
        visited(0, i32::MAX - 1);
    }

    #[test]
    #[should_panic(expected = "loops forever")]
    fn sampling_columns_ending_at_int_max_loop_forever_in_java() {
        visited(i32::MAX - 1, 0);
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
