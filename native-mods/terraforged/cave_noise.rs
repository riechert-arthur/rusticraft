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

use super::terraforged::{Module, cell_vector, floor, value_coord_2d};

/// The jar's `worldgen.cave.CaveType`: how a cave is spread over the world (`GLOBAL` everywhere, `UNIQUE` through
/// `UniqueCaveDistributor`'s cells).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaveType {
    Global,
    Unique,
}

impl CaveType {
    /// `CaveType.values()`, in ordinal order.
    pub const VALUES: [Self; 2] = [Self::Global, Self::Unique];

    /// `getName()`: the lower-case name the cave codec writes.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Global => "global",
            Self::Unique => "unique",
        }
    }

    /// `CaveType.forName`: `valueOf(name.toUpperCase(Locale.ROOT))`. Rust's `to_uppercase` is the same
    /// locale-free Unicode mapping, so `"un\u{131}que"` (dotless i) is `UNIQUE` as in Java. `None` where Java's
    /// `valueOf` throws.
    pub fn for_name(name: &str) -> Option<Self> {
        match name.to_uppercase().as_str() {
            "GLOBAL" => Some(Self::Global),
            "UNIQUE" => Some(Self::Unique),
            _ => None,
        }
    }
}

/// The jar's `worldgen.asset.NoiseCave`: a cave's elevation, shape and floor noise and its vertical range.
#[derive(Clone)]
pub struct NoiseCave {
    seed: i32,
    cave_type: CaveType,
    elevation: Module,
    shape: Module,
    floor: Module,
    size: i32,
    min_y: i32,
    max_y: i32,
    range_y: i32,
}

impl NoiseCave {
    /// `new NoiseCave(seed, type, elevation, shape, floor, size, minY, maxY)`; `rangeY = maxY - minY` wraps like an
    /// `int`.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        seed: i32,
        cave_type: CaveType,
        elevation: Module,
        shape: Module,
        floor: Module,
        size: i32,
        min_y: i32,
        max_y: i32,
    ) -> Self {
        Self {
            seed,
            cave_type,
            elevation,
            shape,
            floor,
            size,
            min_y,
            max_y,
            range_y: max_y.wrapping_sub(min_y),
        }
    }

    /// `withSeed(long)`: reseeds the elevation, shape and floor modules (in that order) through
    /// `Module::with_seed` and keeps everything else, including the cave's own `seed`. Panics where the jar's
    /// `ContextSeedable` throws (see `Module::with_seed`).
    pub fn with_seed(&self, seed: i64) -> Self {
        Self::new(
            self.seed,
            self.cave_type,
            self.elevation.with_seed(seed),
            self.shape.with_seed(seed),
            self.floor.with_seed(seed),
            self.size,
            self.min_y,
            self.max_y,
        )
    }

    pub const fn seed(&self) -> i32 {
        self.seed
    }

    pub const fn cave_type(&self) -> CaveType {
        self.cave_type
    }

    /// `getHeight(x, z)`: `minY` plus the elevation noise scaled to `rangeY`.
    pub fn height(&self, x: i32, z: i32) -> i32 {
        scale_value(x, z, 1.0, self.min_y, self.range_y, &self.elevation)
    }

    /// `getCavernSize(x, z, modifier)`: the shape noise scaled to `size * modifier`.
    pub fn cavern_size(&self, x: i32, z: i32, modifier: f32) -> i32 {
        scale_value(x, z, modifier, 0, self.size, &self.shape)
    }

    /// `getFloorDepth(x, z, size)`: the floor noise scaled to `size`.
    pub fn floor_depth(&self, x: i32, z: i32, size: i32) -> i32 {
        scale_value(x, z, 1.0, 0, size, &self.floor)
    }
}

/// The jar's `worldgen.cave.UniqueCaveDistributor`: jittered cells on staggered rows; a cell whose value is above
/// `density` is 0, the others `1 - sqrt(nearest / second nearest)` squared distance.
#[derive(Clone, Copy)]
pub struct UniqueCaveDistributor {
    seed: i32,
    frequency: f32,
    jitter: f32,
    density: f32,
}

impl UniqueCaveDistributor {
    pub const fn new(seed: i32, frequency: f32, jitter: f32, density: f32) -> Self {
        Self {
            seed,
            frequency,
            jitter,
            density,
        }
    }

    /// `getValue(x, z)`, in the jar's float order. The loops cover `max - 2 ..= max`, which is empty when `max - 2`
    /// wraps (Java's `<=` loop too); NaN distances never win, as in Java.
    pub fn value(&self, mut x: f32, mut z: f32) -> f32 {
        x *= self.frequency;
        z *= self.frequency;
        let max_x = floor(x).wrapping_add(1);
        let max_z = floor(z).wrapping_add(1);
        let mut cell_x = max_x.wrapping_sub(1);
        let mut cell_z = max_z.wrapping_sub(1);
        let mut nearest = f32::INFINITY;
        let mut nearest2 = f32::INFINITY;
        for candidate_z in max_z.wrapping_sub(2)..=max_z {
            let offset_x = (candidate_z & 1) as f32 * 0.5;
            for candidate_x in max_x.wrapping_sub(2)..=max_x {
                let vector = cell_vector(self.seed, candidate_x, candidate_z);
                let point_x = candidate_x as f32 + offset_x + vector[0] * self.jitter;
                let point_z = candidate_z as f32 + vector[1] * self.jitter;
                let delta_x = x - point_x;
                let delta_z = z - point_z;
                let distance = delta_x * delta_x + delta_z * delta_z;
                if distance < nearest {
                    nearest2 = nearest;
                    nearest = distance;
                    cell_x = candidate_x;
                    cell_z = candidate_z;
                } else if distance < nearest2 {
                    nearest2 = distance;
                }
            }
        }
        if self.cell_value(cell_x, cell_z) > self.density {
            0.0
        } else {
            1.0 - (nearest / nearest2).sqrt()
        }
    }

    /// The distributor as a `Module` (the jar's class implements it): `Noise`'s default bounds 0..1. Like any
    /// custom module it can't be reseeded (`with_seed` panics, as the jar has no Cereal spec for it).
    pub fn module(self) -> Module {
        Module::custom(0.0, 1.0, move |x, z| self.value(x, z))
    }

    fn cell_value(&self, cell_x: i32, cell_z: i32) -> f32 {
        (1.0 + value_coord_2d(self.seed, cell_x, cell_z)) * 0.5
    }
}

/// `NoiseCave.getScaleValue`: `minimum + NoiseUtil.floor(noise * range * modifier)`, 0 when `range <= 0`.
fn scale_value(x: i32, z: i32, modifier: f32, minimum: i32, range: i32, noise: &Module) -> i32 {
    if range <= 0 {
        return 0;
    }
    minimum.wrapping_add(floor(
        noise.value(x as f32, z as f32) * range as f32 * modifier,
    ))
}

#[cfg(test)]
mod tests {
    use super::super::terraforged::{Interpolation, Source, SourceConfig, SourceKind};
    use super::super::terraforged_landforms::noise_warp;
    use super::*;
    use std::fmt::Write;

    /// `CaveNoiseOracle`'s output (`.local/terraforged/oracles/cave_noise/`): TerraForged 1.18.2-0.3.1's own
    /// `CaveType`, `UniqueCaveDistributor` and `NoiseCave` classes, run interpreted and JIT-compiled.
    const ORACLE: &str = "\
t global
t global
t global
t unique
t unique
t unique
t -
t -
t -
t -
t -
t unique
u 0 3f800000 e879129090b80b3d 3e57285c 3e57285c 3e4a155c 3e0fdbd8 3eebde26 7fc00000 7fc00000 7fc00000 7fc00000 7fc00000 7fc00000 7fc00000 7fc00000 3e57285c 3e436168 3ebce664 3f531505
g 0 3f800000 99df9dfee8ccfaa3 3c4f272c 3c4f272c 3defc853 0 3ed6415b 7fc00000 7fc00000 7fc00000 7fc00000 7fc00000 7fc00000 7fc00000 7fc00000 3c4f272c 0 3e76ddf2 3f49bb88
u 0 3f800000 1442804059c7918b 0 0 0 0 0 0 0 0 7fc00000 0 7fc00000 0 0 0 0 0 0
g 0 3f800000 47e4526b09a0e1be 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0
u 0 3f800000 7c941fdf9b8955b9 3dd009a0 3dd009a0 0 3e8e6e58 0 7fc00000 0 0 7fc00000 0 7fc00000 0 0 3dd009a0 3dcdd4c0 0 0
g 0 3f800000 6ba5c4f5e04d5d7b 0 0 0 3dc95df3 0 7fc00000 0 0 0 0 0 0 0 0 0 0 0
u 0 3f800000 54db318490696491 3dd009a0 3dd009a0 3f54bf92 3f735f42 3c932f60 7fc00000 7fc00000 7fc00000 7fc00000 7fc00000 7fc00000 7fc00000 7fc00000 3dd009a0 3da2e978 3f309e26 3f1d0fbe
g 0 3f800000 c96e7403e6a67b65 0 0 3f1fd65d 3f6c32ee 3e59b4e8 7fc00000 7fc00000 7fc00000 7fc00000 7fc00000 7fc00000 7fc00000 7fc00000 0 0 3f2e0e71 3ee8a7ee
u 0 3f800000 e46195c042a0f46f 3f800000 3f800000 3f38ff96 0 3f800000 7fc00000 7fc00000 7fc00000 7fc00000 7fc00000 0 0 7fc00000 3f800000 3e95f61a 7fc00000 0
g 0 3f800000 fe4c42a2831e9fa 3f800000 3f800000 3e7de26e 3d546abb 0 7fc00000 7fc00000 7fc00000 7fc00000 7fc00000 7fc00000 7fc00000 7fc00000 3f800000 3e5c3baf 0 0
u 0 3f800000 df1c9dd048c08133 0 0 3e9b3c2c 3e6e5608 0 0 7fc00000 7fc00000 7fc00000 0 7fc00000 0 0 0 0 3eabd036 3f51d54a
g 0 3f800000 c58e47e676c72b8c 0 0 0 3db77949 0 0 7fc00000 7fc00000 0 0 0 0 0 0 0 3e7bb28c 3f38dd4a
u 0 3f800000 284e5635c632b338 3e09f31c 3e09f31c 3f03a4cc 3efe0264 3f3a32f7 0 7fc00000 0 0 0 7fc00000 7fc00000 7fc00000 3e09f31c 3e2cb8f0 0 3f09afb9
g 0 3f800000 556669b9aa624883 0 0 3e8d7f27 3eba70db 3f3b834f 0 7fc00000 0 7fc00000 7fc00000 7fc00000 7fc00000 7fc00000 0 0 0 3edafcca
u 0 3f800000 14d5bceae7b5b1a5 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0
g 0 3f800000 14d5bceae7b5b1a5 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0
c 5852 unique 59d870578541fb88 24,30,11 21,23,11 33,0,10 27,7,11 27,7,11 27,0,11 19,4,11 23,26,11
r 5852 unique 87b8e14d5e56e6a7 24,30,11 16,30,11 19,4,11 32,30,11 32,30,11 19,30,11 27,30,11 24,30,11
r 5852 unique 87b8e14d5e56e6a7 24,30,11 16,30,11 19,4,11 32,30,11 32,30,11 19,30,11 27,30,11 24,30,11
r 5852 unique 7ba3967a1c95afbd 24,30,11 26,30,11 28,25,11 15,30,11 15,30,11 20,30,11 25,30,11 23,30,11
r 5852 unique d0ec55a077044a05 24,30,11 30,0,11 22,30,11 32,0,11 32,0,11 15,30,11 23,30,11 24,26,11
r 5852 unique 2b77172ac4cff97d 24,30,11 20,30,11 28,1,11 15,30,11 15,30,11 20,30,11 26,30,11 24,30,11
r 5852 unique 9f79c8ecc3fccc6f 24,30,11 21,23,11 33,0,10 27,7,11 27,7,11 27,30,11 19,4,11 23,30,11
c 101281 global 47f82724ef4debc9 239,5,11 226,5,11 286,2,11 232,5,11 232,5,11 331,8,11 267,6,11 246,6,11
r 101281 global 62a516335b30f434 239,5,11 226,3,11 210,1,11 292,5,11 292,5,11 292,1,11 218,2,11 241,5,11
r 101281 global 62a516335b30f434 239,5,11 226,3,11 210,1,11 292,5,11 292,5,11 292,1,11 218,2,11 241,5,11
r 101281 global 855a2ac29a73e403 239,5,11 227,3,11 294,5,11 194,5,11 194,5,11 171,7,11 184,8,11 246,6,11
r 101281 global af5d44403fcba45 239,5,11 274,8,11 190,9,11 292,5,11 292,5,11 171,5,11 301,4,11 241,5,11
r 101281 global f62c9f9c2b8cd314 239,5,11 220,5,11 280,5,11 194,5,11 194,5,11 277,7,11 195,3,11 240,5,11
r 101281 global 47f82724ef4debc9 239,5,11 226,5,11 286,2,11 232,5,11 232,5,11 331,8,11 267,6,11 246,6,11
c -5 global c7978789f1b3cd6b 0,0,11 0,0,11 0,0,10 0,0,11 0,0,11 0,0,11 0,0,11 0,0,11
r -5 global 8b8920a6049b27e6 0,0,11 0,0,11 0,0,11 0,0,11 0,0,11 0,0,11 0,0,11 0,0,11
r -5 global 8b8920a6049b27e6 0,0,11 0,0,11 0,0,11 0,0,11 0,0,11 0,0,11 0,0,11 0,0,11
r -5 global 888f09b634cf3aeb 0,0,11 0,0,11 0,0,11 0,0,11 0,0,11 0,0,11 0,0,11 0,0,11
r -5 global 3117421e95a8944a 0,0,11 0,0,11 0,0,11 0,0,11 0,0,11 0,0,11 0,0,11 0,0,11
r -5 global 3dcd6075674aed45 0,0,11 0,0,11 0,0,11 0,0,11 0,0,11 0,0,11 0,0,11 0,0,11
r -5 global c7978789f1b3cd6b 0,0,11 0,0,11 0,0,10 0,0,11 0,0,11 0,0,11 0,0,11 0,0,11
c -2147483648 unique 419fd092d741d8a5 0,0,2 0,0,2 0,0,2 0,0,2 0,0,2 0,0,2 0,0,2 0,0,2
r -2147483648 unique 419fd092d741d8a5 0,0,2 0,0,2 0,0,2 0,0,2 0,0,2 0,0,2 0,0,2 0,0,2
r -2147483648 unique 419fd092d741d8a5 0,0,2 0,0,2 0,0,2 0,0,2 0,0,2 0,0,2 0,0,2 0,0,2
r -2147483648 unique 419fd092d741d8a5 0,0,2 0,0,2 0,0,2 0,0,2 0,0,2 0,0,2 0,0,2 0,0,2
r -2147483648 unique 419fd092d741d8a5 0,0,2 0,0,2 0,0,2 0,0,2 0,0,2 0,0,2 0,0,2 0,0,2
r -2147483648 unique 419fd092d741d8a5 0,0,2 0,0,2 0,0,2 0,0,2 0,0,2 0,0,2 0,0,2 0,0,2
r -2147483648 unique 419fd092d741d8a5 0,0,2 0,0,2 0,0,2 0,0,2 0,0,2 0,0,2 0,0,2 0,0,2
c 9 global 536cdf68b94733a5 -7,-1,2147483647 -7,-1,2147483647 -7,-1,2147483647 -7,-1,2147483647 -7,-1,2147483647 -7,-1,2147483647 -7,-1,2147483647 -7,-1,2147483647
r 9 global 536cdf68b94733a5 -7,-1,2147483647 -7,-1,2147483647 -7,-1,2147483647 -7,-1,2147483647 -7,-1,2147483647 -7,-1,2147483647 -7,-1,2147483647 -7,-1,2147483647
r 9 global 536cdf68b94733a5 -7,-1,2147483647 -7,-1,2147483647 -7,-1,2147483647 -7,-1,2147483647 -7,-1,2147483647 -7,-1,2147483647 -7,-1,2147483647 -7,-1,2147483647
r 9 global 536cdf68b94733a5 -7,-1,2147483647 -7,-1,2147483647 -7,-1,2147483647 -7,-1,2147483647 -7,-1,2147483647 -7,-1,2147483647 -7,-1,2147483647 -7,-1,2147483647
r 9 global 536cdf68b94733a5 -7,-1,2147483647 -7,-1,2147483647 -7,-1,2147483647 -7,-1,2147483647 -7,-1,2147483647 -7,-1,2147483647 -7,-1,2147483647 -7,-1,2147483647
r 9 global 536cdf68b94733a5 -7,-1,2147483647 -7,-1,2147483647 -7,-1,2147483647 -7,-1,2147483647 -7,-1,2147483647 -7,-1,2147483647 -7,-1,2147483647 -7,-1,2147483647
r 9 global 536cdf68b94733a5 -7,-1,2147483647 -7,-1,2147483647 -7,-1,2147483647 -7,-1,2147483647 -7,-1,2147483647 -7,-1,2147483647 -7,-1,2147483647 -7,-1,2147483647
c 1 unique f38e08606a465985 -2147483600,100,10 -2147483600,100,10 -2147483600,100,10 -2147483600,100,10 -2147483600,100,10 -2147483600,100,10 -2147483600,100,10 -2147483600,100,10
r 1 unique f38e08606a465985 -2147483600,100,10 -2147483600,100,10 -2147483600,100,10 -2147483600,100,10 -2147483600,100,10 -2147483600,100,10 -2147483600,100,10 -2147483600,100,10
r 1 unique f38e08606a465985 -2147483600,100,10 -2147483600,100,10 -2147483600,100,10 -2147483600,100,10 -2147483600,100,10 -2147483600,100,10 -2147483600,100,10 -2147483600,100,10
r 1 unique f38e08606a465985 -2147483600,100,10 -2147483600,100,10 -2147483600,100,10 -2147483600,100,10 -2147483600,100,10 -2147483600,100,10 -2147483600,100,10 -2147483600,100,10
r 1 unique f38e08606a465985 -2147483600,100,10 -2147483600,100,10 -2147483600,100,10 -2147483600,100,10 -2147483600,100,10 -2147483600,100,10 -2147483600,100,10 -2147483600,100,10
r 1 unique f38e08606a465985 -2147483600,100,10 -2147483600,100,10 -2147483600,100,10 -2147483600,100,10 -2147483600,100,10 -2147483600,100,10 -2147483600,100,10 -2147483600,100,10
r 1 unique f38e08606a465985 -2147483600,100,10 -2147483600,100,10 -2147483600,100,10 -2147483600,100,10 -2147483600,100,10 -2147483600,100,10 -2147483600,100,10 -2147483600,100,10
";

    const NAMES: [&str; 12] = [
        "global",
        "GLOBAL",
        "Global",
        "unique",
        "UNIQUE",
        "un\u{131}que",
        "GLOBAL ",
        "missing",
        "",
        "\u{ff47}lobal",
        "glob\u{430}l",
        "uniQue",
    ];
    const D_SEED: [i32; 8] = [123, 123, -1, i32::MAX, i32::MIN, 7, 0, 5852 + 1_286_745];
    const D_FREQ: [f32; 8] = [
        0.01,
        0.01,
        1.0 / 600.0,
        1.0 / 30.0,
        1.0,
        0.05,
        -0.02,
        1.0 / 400.0,
    ];
    const D_JITTER: [f32; 8] = [0.75, 0.75, 0.75, 0.75, 0.0, 2.0, 0.75, 0.75];
    const D_DENSITY: [f32; 8] = [1.0, 0.1, 0.25, f32::NAN, 0.5, 0.6, 0.5, -1.0];
    const F_POINTS: [(f32, f32); 17] = [
        (0.0, 0.0),
        (-0.0, -0.0),
        (12.5, -81.25),
        (-450.0, 991.75),
        (2048.0, -4096.0),
        (1e10, -1e10),
        (2_147_483_648.0, -2_147_483_648.0),
        (-2_147_483_648.0, 2_147_483_520.0),
        (f32::INFINITY, 0.0),
        (0.0, f32::NEG_INFINITY),
        (f32::NAN, 3.0),
        (3.0, f32::NAN),
        (3.4e38, -3.4e38),
        (-1e-40, 1e-40),
        (-0.5, -0.5),
        (16_777_216.0, -16_777_217.0),
        (123_456.75, -98_765.5),
    ];
    const I_POINTS: [(i32, i32); 8] = [
        (0, 0),
        (12, -8),
        (-1000, 777),
        (i32::MAX, i32::MIN),
        (i32::MIN, i32::MAX),
        (16_777_217, -16_777_217),
        (123_456_789, -987_654_321),
        (-1, -1),
    ];
    const MODIFIERS: [f32; 8] = [1.0, 0.5, -1.0, 0.0, -0.0, f32::NAN, f32::INFINITY, 1e30];
    const SEEDS: [i64; 6] = [99, 0x1_0000_0063, -1, i64::MIN + 5, i32::MAX as i64, 0];
    const BASIS: u64 = 0xcbf2_9ce4_8422_2325;

    fn gx(i: i32) -> f32 {
        ((i * 7919) % 2001 - 1000) as f32 * 3.7
    }

    fn gz(i: i32) -> f32 {
        ((i * 104_729) % 1999 - 999) as f32 * 5.3
    }

    fn bits(value: f32) -> u32 {
        if value.is_nan() {
            0x7fc0_0000
        } else {
            value.to_bits()
        }
    }

    fn fnv(hash: u64, value: u32) -> u64 {
        value.to_le_bytes().iter().fold(hash, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
        })
    }

    /// `Source.constant(double)`.
    fn c(value: f64) -> Module {
        Module::constant(value as f32)
    }

    fn simplex(seed: i32, frequency: f32, octaves: usize) -> Module {
        Module::source(Source::new(
            SourceKind::Simplex,
            SourceConfig {
                seed,
                octaves,
                gain: 0.5,
                frequency,
                lacunarity: 2.0,
                interpolation: Interpolation::Curve3,
            },
        ))
    }

    fn hash_module(module: &Module) -> String {
        let hash = (0..200).fold(BASIS, |hash, i| fnv(hash, bits(module.value(gx(i), gz(i)))));
        let mut out = format!("{hash:x}");
        for (x, z) in F_POINTS {
            write!(out, " {:x}", bits(module.value(x, z))).unwrap();
        }
        out
    }

    /// `CaveNoiseOracle.cave`: TerraForged's `mega` cave, a `synapse_high`-like one, and edge cases.
    fn cave(index: usize) -> NoiseCave {
        let mega_elevation = simplex(5850, 0.005, 2).map(c(0.3), c(0.7));
        let mega_floor = simplex(5852, 0.02, 2)
            .clamp(c(0.0), c(0.3))
            .map(c(0.0), c(1.0));
        let mega_shape = simplex(5851, 0.004, 3)
            .bias(c(-0.5))
            .abs()
            .scale(c(2.0))
            .invert()
            .clamp(c(0.75), c(1.0))
            .map(c(0.0), c(1.0));
        let high_elevation = simplex(101_278, 0.004, 3).map(c(0.1), c(0.9));
        let high_shape = simplex(101_279, 0.01, 2)
            .warp(noise_warp(101_280, 40, 1, 15.0))
            .map(c(0.0), c(1.0));
        let high_floor = simplex(101_281, 0.03, 1)
            .clamp(c(0.0), c(0.15))
            .map(c(0.0), c(1.0));
        use CaveType::{Global, Unique};
        match index {
            0 => NoiseCave::new(
                5852,
                Unique,
                mega_elevation,
                mega_shape,
                mega_floor,
                30,
                -16,
                64,
            ),
            1 => NoiseCave::new(
                101_281,
                Global,
                high_elevation,
                high_shape,
                high_floor,
                11,
                96,
                384,
            ),
            2 => NoiseCave::new(
                -5,
                Global,
                mega_elevation,
                mega_shape,
                mega_floor,
                0,
                64,
                -16,
            ),
            3 => NoiseCave::new(
                i32::MIN,
                Unique,
                c(0.5),
                c(0.75),
                c(0.25),
                -3,
                i32::MIN,
                i32::MAX,
            ),
            4 => NoiseCave::new(
                9,
                Global,
                c(3e38),
                c(f64::NAN),
                c(-3e38),
                i32::MAX,
                i32::MAX - 5,
                i32::MAX,
            ),
            5 => NoiseCave::new(
                1,
                Unique,
                c(-0.0),
                c(1.0),
                c(f64::from(0.999_999_f32)),
                100,
                -2_147_483_600,
                -100,
            ),
            _ => unreachable!(),
        }
    }

    fn hash_cave(cave: &NoiseCave) -> String {
        let mut hash = BASIS;
        let mut out = String::new();
        for i in 0..64 + I_POINTS.len() as i32 {
            let (x, z) = if i < 64 {
                (gx(i) as i32, gz(i) as i32)
            } else {
                I_POINTS[(i - 64) as usize]
            };
            let height = cave.height(x, z);
            hash = fnv(hash, height as u32);
            for modifier in MODIFIERS {
                let size = cave.cavern_size(x, z, modifier);
                hash = fnv(hash, size as u32);
                hash = fnv(hash, cave.floor_depth(x, z, size) as u32);
            }
            for size in [0, -3, 1, 7, i32::MAX] {
                hash = fnv(hash, cave.floor_depth(x, z, size) as u32);
            }
            if i >= 64 {
                write!(
                    out,
                    " {height},{},{}",
                    cave.cavern_size(x, z, 1.0),
                    cave.floor_depth(x, z, 11)
                )
                .unwrap();
            }
        }
        format!("{hash:x}{out}")
    }

    #[test]
    fn cave_noise_matches_terraforged_0_3_1() {
        let mut out = String::new();
        for name in NAMES {
            writeln!(
                out,
                "t {}",
                CaveType::for_name(name).map_or("-", CaveType::name)
            )
            .unwrap();
        }
        for d in 0..D_SEED.len() {
            let distributor =
                UniqueCaveDistributor::new(D_SEED[d], D_FREQ[d], D_JITTER[d], D_DENSITY[d]);
            let module = distributor.module();
            // `NoiseCaveGenerator.createUniqueNoise`'s graph over it.
            let unique = distributor
                .module()
                .clamp(c(0.2), c(1.0))
                .map(c(0.0), c(1.0))
                .warp(noise_warp(D_SEED[d].wrapping_add(781_624), 30, 1, 20.0));
            for (tag, module) in [("u", &module), ("g", &unique)] {
                writeln!(
                    out,
                    "{tag} {:x} {:x} {}",
                    bits(module.min_value()),
                    bits(module.max_value()),
                    hash_module(module)
                )
                .unwrap();
            }
            for (x, z) in F_POINTS {
                assert_eq!(bits(distributor.value(x, z)), bits(module.value(x, z)));
            }
        }
        for index in 0..6 {
            let cave = cave(index);
            writeln!(
                out,
                "c {} {} {}",
                cave.seed(),
                cave.cave_type().name(),
                hash_cave(&cave)
            )
            .unwrap();
            for seed in SEEDS {
                let reseeded = cave.with_seed(seed);
                writeln!(
                    out,
                    "r {} {} {}",
                    reseeded.seed(),
                    reseeded.cave_type().name(),
                    hash_cave(&reseeded)
                )
                .unwrap();
            }
        }
        assert_eq!(out, ORACLE);
        assert_eq!(CaveType::VALUES.map(CaveType::name), ["global", "unique"]);
    }
}
