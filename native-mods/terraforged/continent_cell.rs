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

use super::terraforged::{Interpolation, cubic, perlin2, simplex};

const X_PRIME: i32 = 1_619;
const Y_PRIME: i32 = 31_337;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CellPoint {
    pub px: f32,
    pub py: f32,
    pub noise: f32,
    pub noise0: f32,
}

impl CellPoint {
    #[allow(clippy::misnamed_getters)]
    pub fn noise(self) -> f32 {
        self.noise0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CellShape {
    Square,
    Hexagon,
}

impl CellShape {
    pub fn adjust_x(self, x: f32) -> f32 {
        x
    }

    pub fn adjust_y(self, y: f32) -> f32 {
        match self {
            Self::Square => y,
            Self::Hexagon => y * 1.2,
        }
    }

    pub fn cell_x(self, hash: i32, cell_x: i32, cell_y: i32, jitter: f32) -> f32 {
        match self {
            Self::Square => position(hash, cell_x, X_PRIME, jitter),
            Self::Hexagon => {
                let offset = (cell_y & 1) as f32 * 0.5;
                let jitter = if offset > 0.0 { jitter * 0.5 } else { jitter };
                position(hash, cell_x, X_PRIME, jitter) + offset
            }
        }
    }

    pub fn cell_y(self, hash: i32, _cell_x: i32, cell_y: i32, jitter: f32) -> f32 {
        position(hash, cell_y, Y_PRIME, jitter)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CellSource {
    Perlin,
    Simplex,
    Cubic,
}

impl CellSource {
    pub fn sample(self, seed: i32, x: f32, y: f32) -> f32 {
        match self {
            Self::Perlin => perlin2(x, y, seed, Interpolation::Curve3),
            Self::Simplex => simplex(x, y, seed),
            Self::Cubic => cubic(x, y, seed),
        }
    }

    pub fn value(self, seed: i32, x: f32, y: f32) -> f32 {
        (1.0 + self.sample(seed, x, y)) * 0.5
    }
}

fn position(hash: i32, cell: i32, prime: i32, jitter: f32) -> f32 {
    cell as f32 + random(hash_1d(hash, prime)) * jitter
}

pub(crate) fn hash_1d(seed: i32, x: i32) -> i32 {
    let mut hash = seed ^ Y_PRIME.wrapping_mul(x);
    hash = hash
        .wrapping_mul(hash)
        .wrapping_mul(hash)
        .wrapping_mul(60_493);
    (hash >> 13) ^ hash
}

#[allow(clippy::manual_clamp)]
pub(crate) fn random(mut value: i32) -> f32 {
    value ^= X_PRIME;
    value ^= Y_PRIME;
    let value = value
        .wrapping_mul(value)
        .wrapping_mul(value)
        .wrapping_mul(60_493) as f32
        / 2_147_483_648.0;
    let clamped = if value < -1.0 {
        -1.0
    } else if value > 1.0 {
        1.0
    } else {
        value
    };
    (clamped + 1.0) / 2.0
}

#[cfg(test)]
mod tests {
    use super::*;

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
