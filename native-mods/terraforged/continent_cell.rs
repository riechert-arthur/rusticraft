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
    terraforged::{Interpolation, cubic, hash, perlin2, simplex},
    terraforged_terrain_blend_util::{X_PRIME, Y_PRIME, random},
};

/// TerraForged 1.18.2-0.3.1's `CellPoint` (`worldgen.noise.continent.cell`): a continent cell's jittered position
/// and its two noise samples. Java's fields start at 0, as `Default` does. `PartialEq` compares fields (Java's
/// `CellPoint` has no `equals`; nothing in the jar compares points).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CellPoint {
    pub px: f32,
    pub py: f32,
    pub noise: f32,
    pub noise0: f32,
}

impl CellPoint {
    /// `CellPoint.noise()`: the jar returns `noise0`, not the `noise` field.
    #[allow(clippy::misnamed_getters)]
    pub fn noise(self) -> f32 {
        self.noise0
    }
}

/// `CellShape`: how continent cells sit on the grid. `SQUARE` jitters each cell inside its own square; `HEXAGON`
/// stretches y by 1.2 and shifts odd rows by half a cell with half the x jitter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CellShape {
    Square,
    Hexagon,
}

impl CellShape {
    /// `CellShape.values()`, in declaration order.
    pub const VALUES: [Self; 2] = [Self::Square, Self::Hexagon];

    /// `adjustX`: the identity for both shapes.
    pub fn adjust_x(self, x: f32) -> f32 {
        x
    }

    /// `adjustY`: `y * 1.2f` for `HEXAGON`, the identity for `SQUARE`.
    pub fn adjust_y(self, y: f32) -> f32 {
        match self {
            Self::Square => y,
            Self::Hexagon => y * 1.2,
        }
    }

    /// `getCellX(hash, cellX, cellY, jitter)`: `MathUtil.getPosX(hash, cellX, jitter)`; `HEXAGON` adds
    /// `(cellY & 1) * 0.5f` and halves the jitter on those odd rows.
    pub fn cell_x(self, hash: i32, cell_x: i32, cell_y: i32, jitter: f32) -> f32 {
        match self {
            Self::Square => pos(hash, X_PRIME, cell_x, jitter),
            Self::Hexagon => {
                let offset = (cell_y & 1) as f32 * 0.5;
                let jitter = if offset > 0.0 { jitter * 0.5 } else { jitter };
                pos(hash, X_PRIME, cell_x, jitter) + offset
            }
        }
    }

    /// `getCellY(hash, cellX, cellY, jitter)`: `MathUtil.getPosY(hash, cellY, jitter)` for both shapes.
    pub fn cell_y(self, hash: i32, _cell_x: i32, cell_y: i32, jitter: f32) -> f32 {
        pos(hash, Y_PRIME, cell_y, jitter)
    }
}

/// `CellSource`: the noise each continent cell samples at its position.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CellSource {
    Perlin,
    Simplex,
    Cubic,
}

impl CellSource {
    /// `CellSource.values()`, in declaration order.
    pub const VALUES: [Self; 3] = [Self::Perlin, Self::Simplex, Self::Cubic];

    /// `sample(seed, x, y)`: `Noise.singlePerlin2(.., CURVE3)`, `Noise.singleSimplex` or `Noise.singleCubic`.
    pub fn sample(self, seed: i32, x: f32, y: f32) -> f32 {
        match self {
            Self::Perlin => perlin2(x, y, seed, Interpolation::Curve3),
            Self::Simplex => simplex(x, y, seed),
            Self::Cubic => cubic(x, y, seed),
        }
    }

    /// `getValue(seed, x, y)`: the sample mapped from -1..1 to 0..1, `(1 + sample) * 0.5f`.
    pub fn value(self, seed: i32, x: f32, y: f32) -> f32 {
        (1.0 + self.sample(seed, x, y)) * 0.5
    }
}

/// `MathUtil.getPosX`/`getPosY(hash, cell, jitter)`: `cell + MathUtil.rand(hash, prime) * jitter`, where
/// `MathUtil.rand(hash, prime)` is `MathUtil.rand(NoiseUtil.hash(hash, prime))`.
fn pos(hash_seed: i32, prime: i32, cell: i32, jitter: f32) -> f32 {
    cell as f32 + random(hash(hash_seed, prime)) * jitter
}

#[cfg(test)]
mod tests {
    use super::*;

    // Expected values come from `ContinentCellOracle.java` (`.local/terraforged/oracles/continent_cell/`), which runs
    // the 1.18.2-0.3.1 jar's own `CellPoint`, `CellShape`, `CellSource`, `MathUtil` and `Noise` bytecode over these
    // inputs: each section is a count and an FNV-1a hash over every result's canonical bits (any NaN as 0x7fc00000).
    const MIN: f32 = f32::from_bits(1);
    const HASHES: [i32; 12] = [
        0,
        1,
        -1,
        17,
        -918_273,
        0x5a5a_5a5a,
        1619,
        31_337,
        i32::MIN,
        i32::MAX,
        123_456_789,
        -2_147_483_647,
    ];
    const CELLS: [i32; 15] = [
        0,
        1,
        -1,
        2,
        -2,
        3,
        -3,
        4,
        -7,
        9,
        11,
        16_777_217,
        -16_777_217,
        i32::MIN,
        i32::MAX,
    ];
    const JITTERS: [f32; 14] = [
        0.0,
        -0.0,
        MIN,
        0.25,
        0.5,
        0.75,
        1.0,
        -1.0,
        2.0,
        1e30,
        f32::MAX,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NAN,
    ];
    const COORDS: [f32; 27] = [
        f32::NEG_INFINITY,
        -f32::MAX,
        -1e30,
        -16_777_217.0,
        -2_147_483_648.0,
        -31.75,
        -1.5,
        -1.0,
        -0.75,
        -MIN,
        -0.0,
        0.0,
        MIN,
        f32::MIN_POSITIVE,
        0.125,
        0.5,
        1.0,
        4.25,
        12.5,
        18.5,
        1234.5678,
        16_777_216.0,
        2_147_483_648.0,
        1e30,
        f32::MAX,
        f32::INFINITY,
        f32::NAN,
    ];
    const SEEDS: [i32; 6] = [0, 1, -1, 918_273, i32::MIN, i32::MAX];

    struct Fnv(u64, usize);

    impl Fnv {
        fn new() -> Self {
            Self(0xcbf2_9ce4_8422_2325, 0)
        }

        fn add(&mut self, value: f32) {
            let bits = if value.is_nan() {
                0x7fc0_0000
            } else {
                value.to_bits()
            };
            for byte in bits.to_le_bytes() {
                self.0 = (self.0 ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
            }
            self.1 += 1;
        }

        fn check(&mut self, name: &str, count: usize, hash: u64) {
            assert_eq!((self.1, self.0), (count, hash), "{name}");
            *self = Self::new();
        }
    }

    #[test]
    fn continent_cell_random_matches_jar() {
        let mut fnv = Fnv::new();
        for prime in [X_PRIME, Y_PRIME] {
            for seed in HASHES {
                fnv.add(random(hash(seed, prime)));
            }
        }
        fnv.check("rand", 24, 0x3a52_72ac_0b9a_296a);
        assert_eq!(
            CellPoint::default(),
            CellPoint {
                px: 0.0,
                py: 0.0,
                noise: 0.0,
                noise0: 0.0
            }
        );
    }

    #[test]
    fn continent_cell_shapes_match_jar() {
        let expected = [
            (0x77f0_834a_80ef_3179, 0x0d8b_6ace_4cfb_95d3),
            (0xc685_68ce_85d4_4922, 0x3d9e_b068_4c5d_4ab7),
        ];
        let mut fnv = Fnv::new();
        for (shape, (adjust, cells)) in CellShape::VALUES.into_iter().zip(expected) {
            COORDS.into_iter().for_each(|c| fnv.add(shape.adjust_x(c)));
            COORDS.into_iter().for_each(|c| fnv.add(shape.adjust_y(c)));
            JITTERS.into_iter().for_each(|j| fnv.add(shape.adjust_y(j)));
            fnv.check(&format!("adjust {shape:?}"), 68, adjust);
            for seed in HASHES {
                for cell_x in CELLS {
                    for cell_y in CELLS {
                        for jitter in JITTERS {
                            fnv.add(shape.cell_x(seed, cell_x, cell_y, jitter));
                            fnv.add(shape.cell_y(seed, cell_x, cell_y, jitter));
                        }
                    }
                }
            }
            fnv.check(&format!("cell {shape:?}"), 75_600, cells);
        }
    }

    #[test]
    fn continent_cell_sources_match_jar() {
        let expected = [
            0xb76f_0b4f_7809_6888,
            0xfacf_13f0_8866_1b5f,
            0x1fb1_c39b_b3b6_8809,
        ];
        let mut fnv = Fnv::new();
        for (source, hash) in CellSource::VALUES.into_iter().zip(expected) {
            for seed in SEEDS {
                for x in COORDS {
                    for y in COORDS {
                        fnv.add(source.sample(seed, x, y));
                        fnv.add(source.value(seed, x, y));
                    }
                }
            }
            fnv.check(&format!("source {source:?}"), 8748, hash);
        }
    }

    #[test]
    fn cell_point_returns_primary_noise() {
        let point = CellPoint {
            noise: 0.75,
            noise0: -0.125,
            ..CellPoint::default()
        };
        assert_eq!(point.noise().to_bits(), 0xbe00_0000);
    }

    #[test]
    fn cell_shapes_match_java_raw_bits() {
        let hashes = [0, 17, -918_273, 0x5a5a_5a5a];
        let cells = [(0, 0), (3, 4), (-7, -3), (11, 9)];
        let jitters = [0.0, 0.25, 0.75, 1.0];
        let expected = [
            [
                (0x0000_0000, 0x0000_0000, 0x0000_0000),
                (0x4040_72b6, 0x4084_4884, 0x3e80_0000),
                (0xc0ce_6692, 0xc010_8c70, 0x3f40_0000),
                (0x4136_4200, 0x411f_495f, 0x3f80_0000),
            ],
            [
                (0x0000_0000, 0x0000_0000, 0x0000_0000),
                (0x4040_72b6, 0x4084_4884, 0x3e99_999a),
                (0xc0c7_3349, 0xc010_8c70, 0x3f66_6667),
                (0x413b_2100, 0x411f_495f, 0x3f99_999a),
            ],
        ];
        for (shape_index, shape) in [CellShape::Square, CellShape::Hexagon]
            .into_iter()
            .enumerate()
        {
            for index in 0..hashes.len() {
                let (cell_x, cell_y) = cells[index];
                let actual = (
                    shape.cell_x(hashes[index], cell_x, cell_y, jitters[index]),
                    shape.cell_y(hashes[index], cell_x, cell_y, jitters[index]),
                    shape.adjust_y(jitters[index]),
                );
                assert_eq!(actual.0.to_bits(), expected[shape_index][index].0);
                assert_eq!(actual.1.to_bits(), expected[shape_index][index].1);
                assert_eq!(actual.2.to_bits(), expected[shape_index][index].2);
            }
        }
    }

    #[test]
    fn cell_sources_match_java_raw_bits() {
        let points = [(0.0, 0.0), (0.125, -0.75), (12.5, 4.25), (-31.75, 18.5)];
        let expected = [
            [
                (0, 0x3f00_0000),
                (0x3d98_4c10, 0x3f09_84c1),
                (0xbd90_908c, 0x3eed_edee),
                (0xbdcd_1fe8, 0x3ee6_5c03),
            ],
            [
                (0, 0x3f00_0000),
                (0xbf0c_83ca, 0x3e66_f86c),
                (0xbe06_533a, 0x3ede_6b32),
                (0x3e84_2fa7, 0x3f21_0bea),
            ],
            [
                (0xbea3_70a8, 0x3eae_47ac),
                (0x3ea4_0cd4, 0x3f29_0335),
                (0xbcfc_6d00, 0x3ef8_1c98),
                (0x3ead_8802, 0x3f2b_6200),
            ],
        ];
        for (source_index, source) in [CellSource::Perlin, CellSource::Simplex, CellSource::Cubic]
            .into_iter()
            .enumerate()
        {
            for (index, (x, y)) in points.into_iter().enumerate() {
                assert_eq!(
                    source.sample(918_273, x, y).to_bits(),
                    expected[source_index][index].0
                );
                assert_eq!(
                    source.value(918_273, x, y).to_bits(),
                    expected[source_index][index].1
                );
            }
        }
    }
}
