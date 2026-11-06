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

use super::terraforged::{
    Interpolation, Module, Source, SourceConfig, SourceKind, cell_vector, floor, value_coord_2d,
};
use super::terraforged_landforms::noise_warp;

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

/// `worldgen.asset.NoiseCave`: a cave's elevation, shape and floor noise and its vertical range. The jar and
/// TerraForged 6dd607eb agree except that 6dd607eb's samplers take the runtime seed (`Seeds.get`, the world seed
/// narrowed to an `int`) and pass it to the noise's `getValue(seed, x, z)`; the samplers here are 6dd607eb's.
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

    /// 6dd607eb's `getHeight(seed, x, z)`: `minY` plus the elevation noise scaled to `rangeY`. `Err` where the noise
    /// can't be sampled seed-first (see [`Module::try_value_seed_first`]). At seed 0 this is the jar's
    /// `getHeight(x, z)` for graphs that sample alike on both paths (no zero or infinite source ranges, where
    /// Noise2D 2cb4843's `NoiseUtil.map` differs).
    pub fn height(&self, seed: i32, x: i32, z: i32) -> Result<i32, &'static str> {
        scale_value(seed, x, z, 1.0, self.min_y, self.range_y, &self.elevation)
    }

    /// 6dd607eb's `getCavernSize(seed, x, z, modifier)`: the shape noise scaled to `size * modifier`.
    pub fn cavern_size(
        &self,
        seed: i32,
        x: i32,
        z: i32,
        modifier: f32,
    ) -> Result<i32, &'static str> {
        scale_value(seed, x, z, modifier, 0, self.size, &self.shape)
    }

    /// 6dd607eb's `getFloorDepth(seed, x, z, size)`: the floor noise scaled to `size`.
    pub fn floor_depth(&self, seed: i32, x: i32, z: i32, size: i32) -> Result<i32, &'static str> {
        scale_value(seed, x, z, 1.0, 0, size, &self.floor)
    }
}

/// 6dd607eb's `NoiseCaveGenerator.createUniqueNoise(500, DENSITY)`: where `UNIQUE` caves form (the modifier
/// `NoiseCaveCarver` scales their cavern size by). Its seeds are fixed; the runtime seed reaches the distributor's
/// cells and the warp's sources through seed-first sampling. (The jar builds it from the context seed instead:
/// `seed + 1286745` for the distributor, `seed + 781624` for the warp.)
pub fn unique_cave_noise() -> Module {
    UniqueCaveDistributor::new(1_286_745, 1.0 / 500.0, 0.75, 0.05)
        .module()
        .clamp(Module::constant(0.2), Module::constant(1.0))
        .map(Module::constant(0.0), Module::constant(1.0))
        .warp(noise_warp(781_624, 30, 1, 20.0))
}

/// 6dd607eb's `NoiseCaveGenerator.createBreachNoise(300, BREACH_THRESHOLD)`: `Source.simplexRidge(1567328, 300, 2)`
/// (the builder's ridge gain 0.975) clamped to `0.7 * 0.8 ..= 0.7` and mapped to 0..1; `CarverChunk.getCarvingMask`
/// reads it. Sample it seed-first with the runtime seed. (The jar seeds the ridge with the context seed `+ 12`.)
pub fn cave_breach_noise() -> Module {
    let threshold = 0.7_f32;
    Module::source(Source::new(
        SourceKind::SimplexRidge,
        SourceConfig {
            seed: 1_567_328,
            octaves: 2,
            gain: 0.975,
            frequency: 1.0 / 300.0,
            lacunarity: 2.0,
            interpolation: Interpolation::Curve3,
        },
    ))
    .clamp(
        Module::constant(threshold * 0.8),
        Module::constant(threshold),
    )
    .map(Module::constant(0.0), Module::constant(1.0))
}

/// `worldgen.cave.UniqueCaveDistributor`: jittered cells on staggered rows; a cell whose value is above `density` is
/// 0, the others `1 - sqrt(nearest / second nearest)` squared distance. 6dd607eb's `getValue(int seed, x, y)` places
/// the cells with the runtime seed (the parameter shadows the field) but rolls their value with the field; the jar's
/// `getValue(x, y)` uses the field for both, which is [`UniqueCaveDistributor::value`] with `self.seed`.
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

    /// 6dd607eb's `getValue(seed, x, z)`, in Java's float order: cell positions from `seed`, the occupancy roll from
    /// the distributor's own seed. The loops cover `max - 2 ..= max`, which is empty when `max - 2` wraps (Java's `<=`
    /// loop too); NaN distances never win, as in Java.
    pub fn value(&self, seed: i32, mut x: f32, mut z: f32) -> f32 {
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
                let vector = cell_vector(seed, candidate_x, candidate_z);
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

    /// The distributor as a `Module` (the class implements it): `Noise`'s default bounds 0..1. Seedless sampling is
    /// the jar's `getValue(x, z)`, seed-first sampling 6dd607eb's `getValue(seed, x, z)`. Like any custom module it
    /// can't be reseeded (`with_seed` panics, as neither tree has a Cereal spec for it).
    pub fn module(self) -> Module {
        Module::seeded_custom(
            0.0,
            1.0,
            move |x, z| self.value(self.seed, x, z),
            move |seed, x, z| self.value(seed, x, z),
        )
    }

    fn cell_value(&self, cell_x: i32, cell_z: i32) -> f32 {
        (1.0 + value_coord_2d(self.seed, cell_x, cell_z)) * 0.5
    }
}

/// 6dd607eb's `NoiseCave.getScaleValue(seed, ...)`: `minimum + NoiseUtil.floor(noise * range * modifier)` (float
/// math, left to right; the `int` sum wraps), 0 when `range <= 0` without sampling.
fn scale_value(
    seed: i32,
    x: i32,
    z: i32,
    modifier: f32,
    minimum: i32,
    range: i32,
    noise: &Module,
) -> Result<i32, &'static str> {
    if range <= 0 {
        return Ok(0);
    }
    let value = noise.try_value_seed_first(seed, x as f32, z as f32)?;
    Ok(minimum.wrapping_add(floor(value * range as f32 * modifier)))
}

#[cfg(test)]
mod tests {
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

    /// `SeededCaveNoiseOracle`'s output (`.local/terraforged/oracles/noise_cave_seeded/`): TerraForged 6dd607eb's own
    /// `UniqueCaveDistributor` and `NoiseCave` (seed-first samplers, `withSeed` through its Cereal round trip) and
    /// `NoiseCaveGenerator`'s `createUniqueNoise`/`createBreachNoise` (copied verbatim) on Noise2D 2cb4843, run
    /// interpreted and JIT-compiled. Folds over the runtime seeds `RUN_SEEDS`.
    const SEEDED_ORACLE: &str = "\
d 0 79af56688a695270 c68d4a9930ca86e 954ffa61ef63f1a b77bff49688f0182 79af56688a695270 fffa76da4fca602a dd5a7eb2f6fd738c be6dc6fced6a99ab
d 1 502b70628f0555fd 26ec013fadc721f 47708e5fd1197c2e e7ff35f90d84716e 502b70628f0555fd e8c3dde9ebcdd23d 4005a2b3b330fd66 9f0ec4be1bb9bf75
d 2 f7890e0bf68ffd7b 8e760a6b03c3aac2 56579e9ae58e68ed 80397edcef15809e f7890e0bf68ffd7b 51fff604998b9684 842344b73576ad0b 683bf69d9ecf91a9
d 3 53d3ed56e2dd4981 f82bc4de8fa49479 bde42d5ed77c7580 73f89d2ab2b97613 53d3ed56e2dd4981 d518f5e2e62b37b4 ccd50c7763821d67 84ec3576216d687c
d 4 db4264f7dda0c68b db4264f7dda0c68b db4264f7dda0c68b db4264f7dda0c68b db4264f7dda0c68b db4264f7dda0c68b db4264f7dda0c68b db4264f7dda0c68b
d 5 501b812ebf662b50 b830d430ed879cd3 505ffae95ef35e17 21f2f6d48137c37c 501b812ebf662b50 32e3049fe549cad3 361904c7260917db cc87fdf2fe6bcbf2
d 6 8218ee41a9a125ea 191f579748e8fcb7 5b8c5a2971bc091d 41733ef7657c4905 8218ee41a9a125ea b7aea1e2767b549e 4d434c879ec174e7 5605c40f4d32f6d2
d 7 5cfed93dc945ff75 5cfed93dc945ff75 5cfed93dc945ff75 5cfed93dc945ff75 5cfed93dc945ff75 5cfed93dc945ff75 5cfed93dc945ff75 5cfed93dc945ff75
m bounds 0 3f800000 0 3f800000
m 0 6c24f8041714cd3e 708 28ca6a9d6fd0da67 2257 -887584,292192,3d91a363 -887487,292131,3f363f78 -887390,292070,3f60dbab
g 0 8e170f1df2de41ff e7a175d43480ceba
m 1 553eb0f99cc197bc 629 da837ad89359429e 2227 -896120,297560,3e455769 -896023,297499,3eb8109b -887584,292192,3e44ed88
g 1 b880b70414ea2d8b 9d141f2058531a1a
m 1337 4dcb4050cbc6dfed 727 59c4f7aa0fe6a2c5 2268 -896217,297621,3d1aa2db -896120,297560,3dda2345 -881570,288410,3e1ee75f
g 1337 2c63af548721d7cf f2b09b35043455f8
m -987654321 1a009c00fcd4053 748 c701baf8c1900f6b 2280 -896314,297682,3f1de064 -896217,297621,3f7dbeba -896120,297560,3f28ef7c
g -987654321 1dd21608deac9ff6 97c0e521f6e9beb
m -2147483648 6c24f8041714cd3e 708 e88ca23a7006792 2243 -887584,292192,3d91a363 -887487,292131,3f363f78 -887390,292070,3f60dbab
g -2147483648 8e170f1df2de41ff e3e13ccb1ed0bba3
m 2147483647 90b21e19d69f5693 682 e390fe5d23bca689 2255 -896314,297682,3ebee265 -896217,297621,3f42a934 -896120,297560,3f670e47
g 2147483647 6f162a08f28a6be3 e3487de280c269
m 1737075661 ac13eef1c740524e 689 a9c1106016931340 2194 -896120,297560,3e25fd3d -896023,297499,3e85672e -895926,297438,3e313149
g 1737075661 3e5382be14333afa 2832bda98778584d
m -594886753 ea7e5bf17fd6cd00 688 d4d3806e3ea644e 2251 -896217,297621,3e784a66 -896120,297560,3ecbd989 -896023,297499,3f138e0b
g -594886753 fa3d6064b69891cb 3ac933a0066c0bd5
c 0 59d870578541fb88 5bbb231aaadb15 246915b396abe19c ac42080d53a108ef 69e396756f6027c2 8469eff622bec578 89943cb1ee7645c8 e775337295e9f85e
r 0 f1ca4301dd8e1075 669671d8427923cd dc4ce14252911ced 87b8e14d5e56e6a7 eb6c5e7a2c221d87 33594d08ee2b22c6 d0ec55a077044a05 942768cfe48ec603 b963646872ae1fec
c 1 47f82724ef4debc9 e0d4d8a0379eed9 cd552aff0bd6712c 83ee8f1aea2918ce 77f126677fa9259e 4acf0d425bb4e827 12992a8c05710411 bac529262c88f472
r 1 d96bcae2ad3e982b b5e8e89afea587a9 46908d07cf2d40d8 62a516335b30f434 6fae0c0d82541ba3 a9a910ee1e4fd3e3 af5d44403fcba45 4197c3f30428681d d421422e9d2d8d14
c 2 c7978789f1b3cd6b ced3e95fb433cd42 8e93238729abbd70 780b8e5c249a87dd 5b19ace44f37e4a5 3dcd6075674aed45 e257b4d0b71d095a 7d00ad4de1b0d870
r 2 69d7c7a5b49b1412 20c15f361ec4d86a f47a8dde69c783e2 8b8920a6049b27e6 36379f34beec3d72 69a91a25dc2a8216 3117421e95a8944a 37a7055331ebade5 6a28b3905a5639e4
c 3 419fd092d741d8a5 419fd092d741d8a5 419fd092d741d8a5 419fd092d741d8a5 419fd092d741d8a5 419fd092d741d8a5 419fd092d741d8a5 419fd092d741d8a5
r 3 419fd092d741d8a5 419fd092d741d8a5 419fd092d741d8a5 419fd092d741d8a5 419fd092d741d8a5 419fd092d741d8a5 419fd092d741d8a5 419fd092d741d8a5 419fd092d741d8a5
c 4 536cdf68b94733a5 536cdf68b94733a5 536cdf68b94733a5 536cdf68b94733a5 536cdf68b94733a5 536cdf68b94733a5 536cdf68b94733a5 536cdf68b94733a5
r 4 536cdf68b94733a5 536cdf68b94733a5 536cdf68b94733a5 536cdf68b94733a5 536cdf68b94733a5 536cdf68b94733a5 536cdf68b94733a5 536cdf68b94733a5 536cdf68b94733a5
c 5 f38e08606a465985 f38e08606a465985 f38e08606a465985 f38e08606a465985 f38e08606a465985 f38e08606a465985 f38e08606a465985 f38e08606a465985
r 5 f38e08606a465985 f38e08606a465985 f38e08606a465985 f38e08606a465985 f38e08606a465985 f38e08606a465985 f38e08606a465985 f38e08606a465985 f38e08606a465985
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
    /// Runtime seeds as `Seeds.get` narrows them (`(int)` of the world seed), including two wider longs.
    const RUN_SEEDS: [i32; 8] = [
        0,
        1,
        1337,
        -987_654_321,
        i32::MIN,
        i32::MAX,
        0x1_2345_6789_abcd_i64 as i32,
        -4_172_117_037_062_373_473_i64 as i32,
    ];
    /// `NoiseCaveGenerator.createArray`'s second copy (`1 * 0xFA90C2L`), then two of the #110 oracle's reseeds.
    const RESEEDS: [i64; 3] = [0xFA_90C2, 0x1_0000_0063, i64::MIN + 5];

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

    /// The oracles' cave fold at one runtime seed: 64 grid points, then `I_POINTS`.
    fn fold_cave(cave: &NoiseCave, seed: i32) -> u64 {
        let mut hash = BASIS;
        for i in 0..64 + I_POINTS.len() as i32 {
            let (x, z) = cave_point(i);
            let height = cave.height(seed, x, z).unwrap();
            hash = fnv(hash, height as u32);
            for modifier in MODIFIERS {
                let size = cave.cavern_size(seed, x, z, modifier).unwrap();
                hash = fnv(hash, size as u32);
                hash = fnv(hash, cave.floor_depth(seed, x, z, size).unwrap() as u32);
            }
            for size in [0, -3, 1, 7, i32::MAX] {
                hash = fnv(hash, cave.floor_depth(seed, x, z, size).unwrap() as u32);
            }
        }
        hash
    }

    fn cave_point(i: i32) -> (i32, i32) {
        if i < 64 {
            (gx(i) as i32, gz(i) as i32)
        } else {
            I_POINTS[(i - 64) as usize]
        }
    }

    /// The jar oracle's cave line: the jar's seedless samplers are 6dd607eb's at seed 0 (these graphs sample alike
    /// on both paths).
    fn hash_cave(cave: &NoiseCave) -> String {
        let mut out = format!("{:x}", fold_cave(cave, 0));
        for i in 64..64 + I_POINTS.len() as i32 {
            let (x, z) = cave_point(i);
            let height = cave.height(0, x, z).unwrap();
            write!(
                out,
                " {height},{},{}",
                cave.cavern_size(0, x, z, 1.0).unwrap(),
                cave.floor_depth(0, x, z, 11).unwrap()
            )
            .unwrap();
        }
        out
    }

    /// The seeded oracle's module fold: 200 grid points, then `F_POINTS`.
    fn fold_seeded(module: &Module, seed: i32) -> String {
        let points = (0..200).map(|i| (gx(i), gz(i))).chain(F_POINTS);
        let hash = points.fold(BASIS, |hash, (x, z)| {
            fnv(hash, bits(module.try_value_seed_first(seed, x, z).unwrap()))
        });
        format!("{hash:x}")
    }

    #[test]
    fn seeded_cave_noise_matches_terraforged_6dd607eb() {
        let mut out = String::new();
        for d in 0..D_SEED.len() {
            let module =
                UniqueCaveDistributor::new(D_SEED[d], D_FREQ[d], D_JITTER[d], D_DENSITY[d])
                    .module();
            write!(out, "d {d}").unwrap();
            for seed in RUN_SEEDS {
                write!(out, " {}", fold_seeded(&module, seed)).unwrap();
            }
            writeln!(out).unwrap();
        }
        let (unique, breach) = (unique_cave_noise(), cave_breach_noise());
        writeln!(
            out,
            "m bounds {:x} {:x} {:x} {:x}",
            bits(unique.min_value()),
            bits(unique.max_value()),
            bits(breach.min_value()),
            bits(breach.max_value())
        )
        .unwrap();
        for seed in RUN_SEEDS {
            let (mut unique_hash, mut breach_hash) = (BASIS, BASIS);
            let (mut hits, mut breaches) = (0, 0);
            let mut raw = String::new();
            // A long scan: unique caves occupy one cell in twenty, each 500 blocks wide.
            for n in 0..20_000 {
                let (x, z) = (n * 97 - 900_000, n * -61 + 300_000);
                let value = unique
                    .try_value_seed_first(seed, x as f32, z as f32)
                    .unwrap();
                let mask = breach
                    .try_value_seed_first(seed, x as f32, z as f32)
                    .unwrap();
                unique_hash = fnv(unique_hash, bits(value));
                breach_hash = fnv(breach_hash, bits(mask));
                if value != 0.0 {
                    if hits < 3 {
                        write!(raw, " {x},{z},{:x}", bits(value)).unwrap();
                    }
                    hits += 1;
                }
                if mask != 0.0 && mask != 1.0 {
                    breaches += 1;
                }
            }
            writeln!(
                out,
                "m {seed} {unique_hash:x} {hits} {breach_hash:x} {breaches}{raw}"
            )
            .unwrap();
            writeln!(
                out,
                "g {seed} {} {}",
                fold_seeded(&unique, seed),
                fold_seeded(&breach, seed)
            )
            .unwrap();
        }
        for index in 0..6 {
            let cave = cave(index);
            write!(out, "c {index}").unwrap();
            for seed in RUN_SEEDS {
                write!(out, " {:x}", fold_cave(&cave, seed)).unwrap();
            }
            write!(out, "\nr {index}").unwrap();
            for reseed in RESEEDS {
                let reseeded = cave.with_seed(reseed);
                for seed in [0, 1337, -987_654_321] {
                    write!(out, " {:x}", fold_cave(&reseeded, seed)).unwrap();
                }
            }
            writeln!(out).unwrap();
        }
        assert_eq!(out, SEEDED_ORACLE);
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
                let jar = distributor.value(D_SEED[d], x, z);
                assert_eq!(bits(jar), bits(module.value(x, z)));
                // Seed-first at the field's own seed is the jar's sample.
                assert_eq!(
                    bits(jar),
                    bits(module.try_value_seed_first(D_SEED[d], x, z).unwrap())
                );
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
