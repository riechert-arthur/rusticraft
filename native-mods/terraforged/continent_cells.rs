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
use std::ops::RangeInclusive;
use std::sync::Mutex;

/// `ContinentGenerator.SAMPLE_SEED_OFFSET`.
const SAMPLE_SEED_OFFSET: i32 = 6569;
/// `ContinentGenerator.CONTINENT_SAMPLE_SCALE`.
const CONTINENT_SAMPLE_SCALE: f32 = 400.0;

/// The cell half of TerraForged 1.18.2-0.3.1's `worldgen.noise.continent.ContinentGenerator` (the jar has no separate
/// cell generator): jittered continent cells with their noise, and the nearest-cell search. The jar's lossy cell
/// cache is transparent (it compares whole keys and its pool always hands out fresh points), so cells are computed
/// directly; the spawn-offset search and the river generator come later.
///
/// Seeds: the jar has no runtime seed. `ContinentNoise.createContinent` binds the world seed by drawing `seed0` and
/// `seed1` from `GeneratorContext.seed`, and every cell uses those. The `seed` parameters here are the newer source's
/// runtime seed, added (Java `int` wrap) to both: the cell hash seed is `seed0 + seed`, the noise seed
/// `seed + seed1 + 6569`. A runtime seed of 0 gives exactly the jar's cells; seed `s` over seeds `(a, b)` equals seed
/// 0 over `(a + s, b + s)`.
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
    /// `new ContinentGenerator(config, levels, controlPoints)`: the cell fields only.
    pub fn new(config: ContinentConfig) -> Self {
        Self {
            seed: config.shape.seed0,
            sample_seed: config.shape.seed1.wrapping_add(SAMPLE_SEED_OFFSET),
            jitter: config.shape.jitter,
            shape: config.shape.cell_shape,
            source: config.shape.cell_source,
            threshold: config.shape.threshold,
            offset: Mutex::new(None),
        }
    }

    /// `computeCell(pack(cellX, cellY), 0, 0, cell)`, which `getCell` caches: the jittered position, then two octaves
    /// of the cell source at 0.1 times it (`sampleCell`: lacunarity 2.75, gain 0.3; `noise0` is the first octave).
    pub fn compute_cell(&self, seed: i32, cell_x: i32, cell_y: i32) -> CellPoint {
        let hash = hash_2d(self.seed.wrapping_add(seed), cell_x, cell_y);
        let px = self.shape.cell_x(hash, cell_x, cell_y, self.jitter);
        let py = self.shape.cell_y(hash, cell_x, cell_y, self.jitter);
        let seed = seed.wrapping_add(self.sample_seed);
        // sampleCell(seed, px, py, source, 2, 400f / 4000f, 2.75f, 0.3f), in its order.
        let frequency = CONTINENT_SAMPLE_SCALE / 4000.0;
        let (mut x, mut y) = (px * frequency, py * frequency);
        let noise0 = self.source.value(seed, x, y);
        let mut sum = noise0;
        let mut amp = 1.0_f32;
        let mut sum_amp = amp;
        amp *= 0.3;
        x *= 2.75;
        y *= 2.75;
        sum += self.source.value(seed, x, y) * amp;
        sum_amp += amp;
        CellPoint {
            px,
            py,
            noise: sum / sum_amp,
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

// World generation shares the generator across threads.
const _: fn() = || {
    fn shared<T: Send + Sync>() {}
    shared::<CellGenerator>();
};

/// `getNearestCell`'s loop bounds: `min..=min + 2` in Java `int` math, empty when the upper bound wraps below.
/// `None` where the jar's loop never ends (upper bound `Integer.MAX_VALUE`: the counter wraps forever); `floor` of an
/// `f32` never returns `Integer.MAX_VALUE - 1`, so this cannot happen from `nearest_cell`.
fn window(min: i32) -> Option<RangeInclusive<i32>> {
    let max = min.wrapping_add(2);
    (max != i32::MAX).then_some(min..=max)
}

impl ContinentCells for CellGenerator {
    /// `getCell`.
    fn cell(&self, seed: i32, x: i32, y: i32) -> CellPoint {
        self.compute_cell(seed, x, y)
    }

    fn cell_shape(&self) -> CellShape {
        self.shape
    }

    /// `getNearestCell`, unpacked: the cell of the 3x3 around `floor(adjusted point)` whose jittered position is
    /// strictly nearest (first wins ties; NaN distances never win). Where `floor` returns `Integer.MAX_VALUE` (huge,
    /// infinite or `-2^31` points) the jar's window wraps and is empty: the answer is cell (0, 0), as in the jar.
    fn nearest_cell(&self, seed: i32, x: f32, y: f32) -> (i32, i32) {
        let x = self.shape.adjust_x(x);
        let y = self.shape.adjust_y(y);
        // The rows loop runs forever past `None`; the columns loop only if a row runs.
        let Some(rows) = window(floor(y).wrapping_sub(1)) else {
            panic!("TerraForged's getNearestCell loops forever at ({x}, {y})");
        };
        let Some(columns) = window(floor(x).wrapping_sub(1)) else {
            assert!(
                rows.is_empty(),
                "TerraForged's getNearestCell loops forever at ({x}, {y})"
            );
            return (0, 0);
        };
        let mut nearest = (0, 0);
        let mut distance = f32::MAX;
        for cell_y in rows {
            for cell_x in columns.clone() {
                let cell = self.cell(seed, cell_x, cell_y);
                // NoiseUtil.dist2(x, y, px, py)
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
        terraforged_engine::{ControlPoints, TerrainRegistry, WorldControlPoints},
        terraforged_terrain_data::NoiseSample,
    };
    use super::*;

    const MIN: f32 = f32::from_bits(1);
    // ContinentCellsOracle's inputs (`.local/terraforged/oracles/continent_cells/`), in its order.
    #[rustfmt::skip]
    const SEEDS: [(i32, i32); 8] = [(0, 0), (1, -1), (i32::MAX, i32::MAX), (i32::MIN, i32::MIN),
        (-1, i32::MAX - 6568), (123_456_789, -987_654_321), (i32::MIN, 6569), (-6569, i32::MIN + 6568)];
    #[rustfmt::skip]
    const JITTERS: [f32; 10] = [0.75, 0.0, -0.0, 1.0, MIN, 1e30, f32::INFINITY, f32::NEG_INFINITY, f32::NAN, -2.5];
    #[rustfmt::skip]
    const COORDS: [i32; 12] = [0, 1, -1, 7, -7, 16_777_217, -16_777_217, 123_456, i32::MAX, i32::MIN,
        i32::MAX - 1, i32::MIN + 1];
    #[rustfmt::skip]
    const POINTS: [f32; 26] = [f32::NAN, f32::NEG_INFINITY, f32::INFINITY, -0.0, 0.0, MIN, -MIN, 1e30, -1e30,
        f32::MAX, -f32::MAX, 2_147_483_520.0, 2_147_483_648.0, -2_147_483_648.0, -2_147_483_520.0, 16_777_216.0,
        -16_777_216.0, 16_777_218.0, 0.5, -0.5, 1.75, -2.25, 123.4, -98.76, 0.999_999_94, -1.0];
    /// The oracle's `fields` lines: `seed`, `sampleSeed` per seed pair.
    #[rustfmt::skip]
    const FIELDS: [(i32, i32); 8] = [(0, 6569), (1, 6568), (2_147_483_647, -2_147_477_080),
        (-2_147_483_648, -2_147_477_079), (-1, -2_147_483_648), (123_456_789, -987_647_752),
        (-2_147_483_648, 13_138), (-6569, -2_147_470_511)];
    /// The oracle's `cells`, `nearest` and `shape` hashes, per (shape, source) in `VALUES` order.
    const CELLS: [u64; 6] = [
        0x26ef_d2f4_f312_2ebd,
        0xb95a_b9fa_4790_8d91,
        0x185e_8157_8915_5dfd,
        0x7c0d_3b53_61e0_8ab5,
        0xa9af_753e_a4c9_bad9,
        0x2ee2_7ad4_55e7_5255,
    ];
    const NEAREST: [u64; 2] = [0x1fe4_804c_8bf7_0d46, 0x44b1_f9e3_a0f4_ccdc];
    const SHAPE: [u64; 6] = [
        0x908c_2dad_9ece_7604,
        0x44ed_6eda_c1bf_f7ca,
        0x0262_36eb_3fe4_255e,
        0x4442_b7a8_ed74_a6a8,
        0x28ae_9349_4310_b7d7,
        0x536b_054a_4e63_7138,
    ];

    struct Fnv(u64, usize);

    impl Fnv {
        fn new() -> Self {
            Self(0xcbf2_9ce4_8422_2325, 0)
        }

        fn bits(&mut self, bits: u32) {
            for byte in bits.to_le_bytes() {
                self.0 = (self.0 ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
            }
            self.1 += 1;
        }

        /// Canonical NaN, as the oracle's `floatToIntBits`: NaN payload signs are not portable.
        fn add(&mut self, value: f32) {
            self.bits(if value.is_nan() {
                0x7fc0_0000
            } else {
                value.to_bits()
            });
        }

        fn cell(&mut self, cell: CellPoint) {
            for value in [cell.px, cell.py, cell.noise, cell.noise0] {
                self.add(value);
            }
        }

        fn check(&mut self, name: &str, count: usize, hash: u64) {
            assert_eq!((self.1, self.0), (count, hash), "{name}");
            *self = Self::new();
        }
    }

    fn config(
        shape: CellShape,
        source: CellSource,
        seeds: (i32, i32),
        jitter: f32,
    ) -> ContinentConfig {
        let mut config = ContinentConfig::default();
        config.shape.cell_shape = shape;
        config.shape.cell_source = source;
        (config.shape.seed0, config.shape.seed1) = seeds;
        config.shape.jitter = jitter;
        config
    }

    fn kinds() -> impl Iterator<Item = (usize, CellShape, CellSource)> {
        CellShape::VALUES
            .into_iter()
            .flat_map(|shape| {
                CellSource::VALUES
                    .into_iter()
                    .map(move |source| (shape, source))
            })
            .enumerate()
            .map(|(index, (shape, source))| (index, shape, source))
    }

    #[test]
    fn seed_fields_match_the_jar() {
        for (seeds, fields) in SEEDS.into_iter().zip(FIELDS) {
            let cells =
                CellGenerator::new(config(CellShape::Square, CellSource::Perlin, seeds, 0.75));
            assert_eq!((cells.seed, cells.sample_seed), fields, "{seeds:?}");
        }
    }

    #[test]
    fn cells_match_the_jar() {
        let mut fnv = Fnv::new();
        for (index, shape, source) in kinds() {
            for seeds in SEEDS {
                for jitter in JITTERS {
                    let cells = CellGenerator::new(config(shape, source, seeds, jitter));
                    // The oracle reads every cell twice, the second time through the jar's cache.
                    for _ in 0..2 {
                        for y in COORDS {
                            for x in COORDS {
                                fnv.cell(cells.cell(0, x, y));
                            }
                        }
                    }
                }
            }
            fnv.check(&format!("cells {index}"), 92_160, CELLS[index]);
        }
    }

    #[test]
    fn nearest_cells_match_the_jar() {
        let mut fnv = Fnv::new();
        for (index, shape, source) in kinds() {
            for seeds in [SEEDS[0], SEEDS[2], SEEDS[4]] {
                for jitter in JITTERS {
                    let cells = CellGenerator::new(config(shape, source, seeds, jitter));
                    for y in POINTS {
                        for x in POINTS {
                            let (cell_x, cell_y) = cells.nearest_cell(0, x, y);
                            fnv.bits(cell_x as u32);
                            fnv.bits(cell_y as u32);
                        }
                    }
                }
            }
            fnv.check(&format!("nearest {index}"), 40_560, NEAREST[index / 3]);
        }
    }

    #[test]
    fn shapes_over_cells_match_the_jar() {
        let terrains = TerrainRegistry::new();
        let mut fnv = Fnv::new();
        for (index, shape, source) in kinds() {
            for seeds in [SEEDS[1], SEEDS[3], SEEDS[5]] {
                for jitter in [0.75, 0.0, 1.0, f32::NAN] {
                    let config = config(shape, source, seeds, jitter);
                    let cells = CellGenerator::new(config);
                    let generator = ShapeGenerator::new(
                        config,
                        ControlPoints::new(WorldControlPoints::default()),
                    );
                    for y in POINTS {
                        for x in POINTS {
                            let mut sample = NoiseSample::new(&terrains);
                            generator.sample(&cells, 0, x, y, &mut sample);
                            fnv.add(sample.base_noise);
                            fnv.add(sample.continent_noise);
                            fnv.add(generator.value(&cells, 0, x, y));
                        }
                    }
                }
            }
            fnv.check(&format!("shape {index}"), 24_336, SHAPE[index]);
        }
    }

    /// The runtime seed is added to both of the jar's seeds: seed `s` over `(a, b)` is seed 0 over `(a + s, b + s)`.
    #[test]
    fn runtime_seed_shifts_both_jar_seeds() {
        for (index, shape, source) in kinds() {
            for (seeds, seed) in
                SEEDS
                    .into_iter()
                    .zip([0, 1, -1, i32::MAX, i32::MIN, 918_273, 6569, -6569])
            {
                let seeded = CellGenerator::new(config(shape, source, seeds, 0.75));
                let shifted = (seeds.0.wrapping_add(seed), seeds.1.wrapping_add(seed));
                let jar = CellGenerator::new(config(shape, source, shifted, 0.75));
                for (x, y) in [(0, 0), (-7, 3), (i32::MAX, i32::MIN)] {
                    assert_eq!(
                        seeded.cell(seed, x, y),
                        jar.cell(0, x, y),
                        "{index} {seeds:?} {seed}"
                    );
                }
                for (x, y) in [(0.5, -0.25), (-1.0, -0.75), (123.4, -98.76)] {
                    let point = (x, y);
                    assert_eq!(
                        seeded.nearest_cell(seed, x, y),
                        jar.nearest_cell(0, x, y),
                        "{point:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn window_wraps_like_java_loops() {
        assert_eq!(window(-1), Some(-1..=1));
        assert!(window(i32::MAX - 1).unwrap().is_empty());
        assert!(window(i32::MAX).unwrap().is_empty());
        assert_eq!(window(i32::MAX - 2), None);
        assert_eq!(window(i32::MIN), Some(i32::MIN..=i32::MIN + 2));
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
