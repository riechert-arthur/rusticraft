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
    terraforged::{clamp, map, map_current},
    terraforged_continent_cell::{CellPoint, CellShape},
    terraforged_continent_config::ContinentConfig,
    terraforged_continent_points::{FalloffPoint, falloff, falloff_current, falloff_curve},
    terraforged_engine::ControlPoints,
    terraforged_terrain_data::NoiseSample,
};
use std::ops::RangeInclusive;

/// What TerraForged 1.18.2-0.3.1's `ShapeGenerator` reads from its `ContinentGenerator`: `getNearestCell` (unpacked
/// as `PosUtil.unpackLeft`/`unpackRight`), `getCell` and the `cellShape` field. The jar binds its seeds inside the
/// generator; `seed` is the world's runtime seed that `CellGenerator` (`continent_cells.rs`) adds to them (the newer source's
/// seed-first style). `ShapeGenerator` only passes it through.
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

/// The jar's `ShapeGenerator` (`worldgen.noise.continent.shape`): blends the cells around a point into its base and
/// continent noise. The continent is passed to each call instead of stored; the jar's thread-local buffers are stack
/// arrays. `threshold`, `base_falloff_min` and `base_falloff_max` are the jar's public fields.
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
    /// `new ShapeGenerator(continent, config, controlPoints)`: both base falloffs are offsets from the threshold.
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

    /// `getThresholdValue`: 0 below the threshold, else 1 (NaN included). Reads the `noise` field, not `noise()`.
    pub fn threshold_value(&self, cell: CellPoint) -> f32 {
        if cell.noise < self.threshold {
            0.0
        } else {
            1.0
        }
    }

    /// `getFalloff`: `ContinentPoints.getFalloff` over the control points' curve.
    pub fn falloff(&self, continent_noise: f32) -> f32 {
        falloff(continent_noise, &self.falloff_points)
    }

    /// `getBaseNoise`: `NoiseUtil.map(value, min, max, max - min)`.
    pub fn base_noise(&self, value: f32) -> f32 {
        let (min, max) = (self.base_falloff_min, self.base_falloff_max);
        map(value, min, max, max - min)
    }

    /// The newer source's `getFalloff` (same bytecode as the jar's): [`falloff_current`], Noise2D 2cb4843's map.
    pub fn falloff_current(&self, continent_noise: f32) -> f32 {
        falloff_current(continent_noise, &self.falloff_points)
    }

    /// The newer source's `getBaseNoise` (same bytecode as the jar's) through Noise2D 2cb4843's `NoiseUtil.map`
    /// ([`map_current`]): 1 instead of NaN where the base falloffs are equal or infinitely apart.
    pub fn base_noise_current(&self, value: f32) -> f32 {
        let (min, max) = (self.base_falloff_min, self.base_falloff_max);
        map_current(value, min, max, max - min)
    }

    /// `getValue(x, y)`: the continent falloff of the 3x3 cells' blended threshold values, clamped to 0..1 before
    /// the falloff. Nothing in the jar calls it. Panics where the cell window wraps past `i32` (see [`window`]).
    pub fn value(&self, continent: &impl ContinentCells, seed: i32, x: f32, y: f32) -> f32 {
        let (center_x, center_y) = continent.nearest_cell(seed, x, y);
        let shape = continent.cell_shape();
        let (x, y) = (shape.adjust_x(x), shape.adjust_y(y));
        let mut edges = [(0.0, 0.0); 9];
        let mut index = 0;
        let (mut minimum0, mut minimum1) = (f32::MAX, f32::MAX);
        for cell_y in window(center_y, 1) {
            for cell_x in window(center_x, 1) {
                let cell = continent.cell(seed, cell_x, cell_y);
                let distance = distance(x, y, cell);
                edges[index] = (self.threshold_value(cell), distance);
                index += 1;
                (minimum0, minimum1) = nearest(distance, minimum0, minimum1);
            }
        }
        let blend = (minimum0 + minimum1) * 0.5 * self.continent_falloff;
        let (mut sum, mut weights) = (0.0, 0.0);
        for (value, distance) in edges {
            let weight = weight(distance, minimum0, blend);
            sum += value * weight;
            weights += weight;
        }
        self.falloff(clamp(sum / weights, 0.0, 1.0))
    }

    /// `sample(x, y, sample)`: blends the 5x5 cells around the nearest one into `sample.base_noise` (the cells'
    /// `noise()` through [`Self::base_noise`]) and `sample.continent_noise` (their threshold values through
    /// [`Self::falloff`]); other fields are left as they are. Panics where the cell window wraps past `i32`.
    pub fn sample(
        &self,
        continent: &impl ContinentCells,
        seed: i32,
        x: f32,
        y: f32,
        sample: &mut NoiseSample,
    ) {
        self.sample_with(continent, seed, x, y, sample, false);
    }

    /// The newer source's `sample(seed, x, y, sample)` (6dd607eb, the jar's bytecode with the seed passed down) as it
    /// runs on Noise2D 2cb4843: [`Self::sample`] through [`Self::base_noise_current`] and [`Self::falloff_current`].
    /// The two differ only where a map range is zero or infinite (equal base falloffs, an inland point of 1).
    pub fn sample_current(
        &self,
        continent: &impl ContinentCells,
        seed: i32,
        x: f32,
        y: f32,
        sample: &mut NoiseSample,
    ) {
        self.sample_with(continent, seed, x, y, sample, true);
    }

    /// [`Self::sample`] (`current` false) or [`Self::sample_current`].
    fn sample_with(
        &self,
        continent: &impl ContinentCells,
        seed: i32,
        x: f32,
        y: f32,
        sample: &mut NoiseSample,
        current: bool,
    ) {
        let (center_x, center_y) = continent.nearest_cell(seed, x, y);
        let shape = continent.cell_shape();
        let (x, y) = (shape.adjust_x(x), shape.adjust_y(y));
        let mut cells = [(CellPoint::default(), 0.0); 25];
        let mut index = 0;
        let (mut minimum0, mut minimum1) = (f32::MAX, f32::MAX);
        for cell_y in window(center_y, 2) {
            for cell_x in window(center_x, 2) {
                let cell = continent.cell(seed, cell_x, cell_y);
                let distance = distance(x, y, cell);
                cells[index] = (cell, distance);
                index += 1;
                (minimum0, minimum1) = nearest(distance, minimum0, minimum1);
            }
        }
        self.sample_edges(minimum0, minimum1, &cells, sample, current);
    }

    /// `sampleEdges`: the jar also passes the nearest cell's index, which it never reads.
    fn sample_edges(
        &self,
        minimum0: f32,
        minimum1: f32,
        cells: &[(CellPoint, f32); 25],
        sample: &mut NoiseSample,
        current: bool,
    ) {
        let border_distance = (minimum0 + minimum1) * 0.5;
        let base_blend = border_distance * self.base_falloff;
        let continent_blend = border_distance * self.continent_falloff;
        let mut sum_base = 0.0;
        let mut sum_continent = 0.0;
        let mut base_weights = 0.0;
        let mut continent_weights = 0.0;
        for &(cell, distance) in cells {
            let base_weight = weight(distance, minimum0, base_blend);
            let continent_weight = weight(distance, minimum0, continent_blend);
            sum_base += cell.noise() * base_weight;
            sum_continent += self.threshold_value(cell) * continent_weight;
            base_weights += base_weight;
            continent_weights += continent_weight;
        }
        let (base, continent) = (sum_base / base_weights, sum_continent / continent_weights);
        if current {
            sample.base_noise = self.base_noise_current(base);
            sample.continent_noise = self.falloff_current(continent);
        } else {
            sample.base_noise = self.base_noise(base);
            sample.continent_noise = self.falloff(continent);
        }
    }
}

/// Java's `for (int c = center - radius; c <= center + radius; c++)`. The jar's loops visit the `2 * radius + 1`
/// cells only while neither bound wraps and the upper one is below `i32::MAX`. Otherwise its loop is empty (it then
/// blends whatever its thread-local buffer holds: a `NullPointerException` or zeros on a fresh thread, a previous
/// call's cells after that), overruns the buffer (`ArrayIndexOutOfBoundsException`) or never ends. None of that is
/// representable, so this panics; it takes cells within `radius` of the `i32` range, ~2^31 cells from the origin.
fn window(center: i32, radius: i32) -> RangeInclusive<i32> {
    match (center.checked_sub(radius), center.checked_add(radius)) {
        (Some(min), Some(max)) if max < i32::MAX => min..=max,
        _ => panic!("TerraForged's continent cell window wraps past i32 at cell {center}"),
    }
}

/// `NoiseUtil.sqrt(NoiseUtil.dist2(x, y, cell.px, cell.py))`: the square root in `f64`, as `Math.sqrt`.
fn distance(x: f32, y: f32, cell: CellPoint) -> f32 {
    let (delta_x, delta_y) = (x - cell.px, y - cell.py);
    f64::from(delta_x * delta_x + delta_y * delta_y).sqrt() as f32
}

/// The jar's running two nearest distances (`<`: a NaN distance changes neither).
fn nearest(distance: f32, minimum0: f32, minimum1: f32) -> (f32, f32) {
    if distance < minimum0 {
        (distance, minimum0)
    } else if distance < minimum1 {
        (minimum0, distance)
    } else {
        (minimum0, minimum1)
    }
}

/// `getWeight`: 1 up to `origin`, 0 from `origin + blend_range`, linear between; NaN passes through.
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
    use super::super::terraforged_engine::{TerrainRegistry, WorldControlPoints};
    use super::*;
    use std::cell::{Cell, RefCell};

    const MIN: f32 = f32::from_bits(1);
    // The oracle's inputs, in its order.
    #[rustfmt::skip]
    const EDGES: [f32; 16] = [f32::NAN, f32::NEG_INFINITY, -f32::MAX, -1.0, -0.0, 0.0, MIN, f32::MIN_POSITIVE, 0.25,
        0.5, 0.999_999_94, 1.0, 1.000_000_1, 2.0, f32::MAX, f32::INFINITY];
    const PLAIN_OFFSETS: [f32; 8] = [0.0, 0.25, 0.5, 0.75, -0.25, 0.1, 0.9, 0.333];
    #[rustfmt::skip]
    const FAR_OFFSETS: [f32; 9] = [0.0, 0.5, -0.0, f32::INFINITY, f32::NEG_INFINITY, 1e30, MIN, -0.75, 0.25];
    #[rustfmt::skip]
    const EDGE_OFFSETS: [f32; 9] = [0.0, 0.5, -0.0, f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 1e30, MIN, -0.75];
    const RARE_EDGES: [f32; 4] = [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, f32::MAX];
    const PLAIN_NOISES: [f32; 8] = [0.0, 0.1, 0.3, 0.5, 0.6, 0.75, 0.9, 1.0];
    const OFFSETS: [f32; 6] = [-2.5, -0.5, 0.0, 0.3, 0.5, 1.75];
    #[rustfmt::skip]
    const POINTS: [f32; 8] = [f32::NAN, f32::NEG_INFINITY, f32::INFINITY, -0.0, 0.0, -1e30, f32::MAX, MIN];
    #[rustfmt::skip]
    const CENTERS: [(i32, i32); 5] = [(0, 0), (-1, -1), (5, -7), (-123_456, 16_777_217),
        (i32::MIN + 2, i32::MAX - 3)];
    /// `ContinentShapeOracle` rerun with Noise2D 2cb4843's `NoiseUtil.map` first on the class path (`out-current.txt`,
    /// the newer source's semantics: its `ShapeGenerator` is the jar's bytecode with the seed passed down): only config
    /// 2 (equal base falloffs, a zero map range) changes, its `single` and `blend` hashes. Its `getValue` (jar only,
    /// still [`ShapeGenerator::value`] there) reads only the falloff, whose curves have no zero-width band here.
    const CURRENT_2: (u64, u64) = (0xe6ec_1cd8_d873_9cd1, 0x973a_4161_e0d8_0038);
    /// `ContinentShapeOracle` per config: derived threshold and base falloffs, then the `single` and `blend` hashes.
    const EXPECTED: [([u32; 3], u64, u64); 9] = [
        (
            [0x3f06_6666, 0x3f08_f5c2, 0x3f46_6666],
            0xb21c_5cb6_4e2b_6bb9,
            0xb48b_3f9c_68f5_21f8,
        ),
        (
            [0x3f06_6666, 0x3f08_f5c2, 0x3f46_6666],
            0xb21c_5cb6_4e2b_6bb9,
            0x62b1_8119_9940_3237,
        ),
        ([0, 0, 0], 0xd18b_80ae_c599_e3d1, 0x86be_b63c_e79a_32b8),
        (
            [0x7fc0_0000, 0x7fc0_0000, 0x7fc0_0000],
            0x24cb_c505_4288_c0e1,
            0xe924_fac6_f825_ee25,
        ),
        (
            [0xff80_0000, 0xff80_0000, 0xffc0_0000],
            0xf69e_c1e0_c59b_c1d1,
            0x81e7_e340_7cf1_d225,
        ),
        (
            [0x3ecc_cccd, 0x3f33_3334, 0x3f00_0000],
            0xa0d5_e631_fc48_70b5,
            0xd83c_d4fa_08bd_9144,
        ),
        (
            [0x3f80_0000, 0x7149_f2ca, 0xf149_f2ca],
            0xa05a_aeb7_13e6_b4b5,
            0x05c0_6e5f_a283_399d,
        ),
        (
            [0x3ecc_cccd, 0x3ee6_6667, 0x3f66_6666],
            0x87f7_3fc1_f27e_5afc,
            0x0bc6_00fc_593f_86d4,
        ),
        (
            [0x3f06_6667, 0x3f08_f5c3, 0x3f46_6668],
            0xc957_e2fb_66f8_a357,
            0xd033_080e_d0e2_bfad,
        ),
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
        }

        /// Canonical NaN, as the oracle's `floatToIntBits`: NaN payload signs are not portable.
        fn add(&mut self, value: f32) {
            self.bits(if value.is_nan() {
                0x7fc0_0000
            } else {
                value.to_bits()
            });
            self.1 += 1;
        }

        fn check(&mut self, name: &str, count: usize, hash: u64) {
            assert_eq!((self.1, self.0), (count, hash), "{name}");
            *self = Self::new();
        }
    }

    /// The oracle's stub `ContinentGenerator`: a fixed nearest cell and a hashed table of cells.
    struct Cells {
        shape: CellShape,
        center: Cell<(i32, i32)>,
        offsets: Vec<f32>,
        noises: Vec<f32>,
        visits: RefCell<Fnv>,
    }

    impl ContinentCells for Cells {
        fn nearest_cell(&self, _seed: i32, _x: f32, _y: f32) -> (i32, i32) {
            self.center.get()
        }

        fn cell(&self, _seed: i32, x: i32, y: i32) -> CellPoint {
            let mut visits = self.visits.borrow_mut();
            visits.bits(x as u32);
            visits.bits(y as u32);
            visits.1 += 1;
            let mut h = x
                .wrapping_mul(0x27d4_eb2d)
                .wrapping_add(y.wrapping_mul(0x1656_67b1)) as u32;
            h ^= h >> 15;
            h = h.wrapping_mul(0x85eb_ca6b);
            h ^= h >> 13;
            let pick = |list: &[f32], byte: u32| list[(byte % list.len() as u32) as usize];
            CellPoint {
                px: x as f32 + pick(&self.offsets, h & 0xff),
                py: y as f32 + pick(&self.offsets, (h >> 8) & 0xff),
                noise: pick(&self.noises, (h >> 16) & 0xff),
                noise0: pick(&self.noises, h >> 24),
            }
        }

        fn cell_shape(&self) -> CellShape {
            self.shape
        }
    }

    fn config(threshold: f32, min: f32, max: f32, base: f32, continent: f32) -> ContinentConfig {
        let mut config = ContinentConfig::default();
        config.shape.threshold = threshold;
        config.shape.base_falloff_min = min;
        config.shape.base_falloff_max = max;
        config.noise.base_noise_falloff = base;
        config.noise.continent_noise_falloff = continent;
        config
    }

    fn points(deep: f32, shallow: f32, beach: f32, coast: f32, inland: f32) -> ControlPoints {
        ControlPoints::new(WorldControlPoints {
            deep_ocean: deep,
            shallow_ocean: shallow,
            beach,
            coast,
            inland,
        })
    }

    fn around(values: &[f32]) -> Vec<f32> {
        values
            .iter()
            .flat_map(|&value| [value.next_down(), value, value.next_up()])
            .collect()
    }

    /// The jar's outputs, then the current-source variants' (`sample_current`, `base_noise_current`, `falloff_current`).
    #[test]
    fn continent_shape_matches_jar_and_newer_source() {
        let mut fnv = Fnv::new();
        for distance in EDGES {
            for origin in EDGES {
                for range in EDGES {
                    fnv.add(weight(distance, origin, range));
                }
            }
        }
        fnv.check("weight", 4096, 0xc9f2_231b_7f0b_ec23);

        let configs = [
            ContinentConfig::default(),
            config(0.525, 0.01, 0.25, 0.0, 0.0),
            config(0.0, 0.0, 0.0, 1.5, 1.0),
            config(f32::NAN, 0.01, 0.25, 1.5, 1.0),
            config(
                f32::NEG_INFINITY,
                -0.0,
                f32::INFINITY,
                f32::INFINITY,
                f32::NAN,
            ),
            config(0.4, 0.3, 0.1, -1.0, 0.5),
            config(1.0, 1e30, -1e30, MIN, f32::MAX),
            config(0.4, 0.05, 0.5, 0.25, 4.0),
            config(
                0.525f32.next_up(),
                0.01f32.next_down(),
                0.25f32.next_up(),
                1.5,
                1.0,
            ),
        ];
        let mut controls = [ControlPoints::default(); 9];
        controls[7] = points(0.05, 0.2, 0.3, 0.4, 0.6);
        controls[8] = points(0.5, 0.4, 0.3, 0.2, 0.1);
        let terrains = TerrainRegistry::new();
        for (current, (index, (config, (derived, mut single, mut blend)))) in
            [false, true].into_iter().flat_map(|current| {
                configs
                    .into_iter()
                    .zip(EXPECTED)
                    .enumerate()
                    .map(move |c| (current, c))
            })
        {
            if current && index == 2 {
                (single, blend) = CURRENT_2;
            }
            let generator = ShapeGenerator::new(config, controls[index]);
            let (base_noise, falloff): (
                fn(&ShapeGenerator, f32) -> f32,
                fn(&ShapeGenerator, f32) -> f32,
            ) = if current {
                (
                    ShapeGenerator::base_noise_current,
                    ShapeGenerator::falloff_current,
                )
            } else {
                (ShapeGenerator::base_noise, ShapeGenerator::falloff)
            };
            let (threshold, min, max) = (
                generator.threshold,
                generator.base_falloff_min,
                generator.base_falloff_max,
            );
            assert_eq!(
                [threshold.to_bits(), min.to_bits(), max.to_bits()],
                derived,
                "derived {index}"
            );
            let singles = around(&[threshold, min, max, 0.1, 0.25, 0.327, 0.448, 0.502]);
            for value in EDGES.into_iter().chain(singles) {
                let cell = CellPoint {
                    noise: value,
                    noise0: -value,
                    ..CellPoint::default()
                };
                fnv.add(generator.threshold_value(cell));
                fnv.add(base_noise(&generator, value));
                fnv.add(falloff(&generator, value));
            }
            fnv.check(&format!("single {index}"), 120, single);

            let noises: Vec<_> = PLAIN_NOISES
                .into_iter()
                .chain(around(&[threshold, min, max]))
                .collect();
            let rare: Vec<_> = noises.repeat(4).into_iter().chain(RARE_EDGES).collect();
            let edges: Vec<_> = noises.iter().copied().chain(EDGES).collect();
            let tables: [(&[f32], &[f32]); 4] = [
                (&PLAIN_OFFSETS, &noises),
                (&FAR_OFFSETS, &noises),
                (&PLAIN_OFFSETS, &rare),
                (&EDGE_OFFSETS, &edges),
            ];
            let visits = RefCell::new(Fnv::new());
            for shape in CellShape::VALUES {
                for (offsets, noises) in tables {
                    let cells = Cells {
                        shape,
                        center: Cell::new((0, 0)),
                        offsets: offsets.to_vec(),
                        noises: noises.to_vec(),
                        visits: RefCell::new(visits.replace(Fnv::new())),
                    };
                    let mut blend = |x: f32, y: f32| {
                        let mut sample = NoiseSample::new(&terrains);
                        if current {
                            generator.sample_current(&cells, 0, x, y, &mut sample);
                        } else {
                            generator.sample(&cells, 0, x, y, &mut sample);
                        }
                        fnv.add(sample.base_noise);
                        fnv.add(sample.continent_noise);
                        fnv.add(generator.value(&cells, 0, x, y));
                    };
                    for (center_x, center_y) in CENTERS {
                        cells.center.set((center_x, center_y));
                        let (x, y) = (center_x as f32, center_y as f32);
                        for dy in OFFSETS {
                            for dx in OFFSETS {
                                blend(x + dx, y + dy);
                            }
                        }
                        for point in POINTS {
                            blend(point, y);
                            blend(x, point);
                        }
                    }
                    visits.replace(cells.visits.into_inner());
                }
            }
            fnv.check(&format!("blend {index}"), 6240, blend);
            visits
                .into_inner()
                .check("visits", 70_720, 0x8738_ee44_c85a_47e5);
        }
    }

    /// Noise2D 2cb4843's `NoiseUtil.map` (`dif >= range ? 1 : dif / range`) against the jar's (`dif > range`): they
    /// part only where the range is zero (equal base falloffs) or infinite (0/0 and inf/inf are NaN in the jar).
    #[test]
    fn current_base_noise_differs_only_at_zero_and_infinite_ranges() {
        let base = |min, max| {
            ShapeGenerator::new(config(0.0, min, max, 1.0, 1.0), ControlPoints::default())
        };
        for (generator, values) in [
            (base(0.1, 0.1), [0.0, 0.1, 0.5, f32::INFINITY]),
            (
                base(f32::NEG_INFINITY, f32::INFINITY),
                [0.0, -1.0, 1e30, f32::MAX],
            ),
        ] {
            for value in values {
                assert!(generator.base_noise(value).is_nan(), "{value}");
                assert_eq!(generator.base_noise_current(value), 1.0, "{value}");
            }
            // NaN passes through `clamp` in both.
            assert!(generator.base_noise_current(f32::NAN).is_nan());
        }
        let finite = base(0.01, 0.25);
        for value in EDGES.into_iter().chain(PLAIN_NOISES) {
            let (jar, current) = (finite.base_noise(value), finite.base_noise_current(value));
            assert_eq!(jar.to_bits(), current.to_bits(), "{value}");
        }
    }

    #[test]
    fn sampling_keeps_other_sample_fields() {
        let generator = ShapeGenerator::new(ContinentConfig::default(), ControlPoints::default());
        let cells = Cells {
            shape: CellShape::Hexagon,
            center: Cell::new((3, -4)),
            offsets: PLAIN_OFFSETS.to_vec(),
            noises: PLAIN_NOISES.to_vec(),
            visits: RefCell::new(Fnv::new()),
        };
        let terrains = TerrainRegistry::new();
        let mut sample = NoiseSample::new(&terrains);
        sample.height_noise = 0.123;
        sample.river_noise = 0.456;
        sample.continent_center = 789;
        generator.sample(&cells, 42, 3.25, -4.75, &mut sample);
        assert_eq!(
            (
                sample.height_noise,
                sample.river_noise,
                sample.continent_center
            ),
            (0.123, 0.456, 789)
        );
        assert_eq!(sample.terrain_type.id(), 0);
    }

    /// The jar overruns its 25-cell buffer here (`ArrayIndexOutOfBoundsException`, oracle `wrap` line).
    #[test]
    #[should_panic(expected = "wraps past i32")]
    fn sampling_panics_where_the_cell_window_wraps() {
        let generator = ShapeGenerator::new(ContinentConfig::default(), ControlPoints::default());
        let cells = Cells {
            shape: CellShape::Square,
            center: Cell::new((i32::MAX - 2, 0)),
            offsets: PLAIN_OFFSETS.to_vec(),
            noises: PLAIN_NOISES.to_vec(),
            visits: RefCell::new(Fnv::new()),
        };
        // The 3x3 window still fits, as in the jar (oracle: getValue returns 1).
        assert_eq!(generator.value(&cells, 0, 0.0, 0.0), 1.0);
        generator.sample(
            &cells,
            0,
            0.0,
            0.0,
            &mut NoiseSample::new(&TerrainRegistry::new()),
        );
    }
}
