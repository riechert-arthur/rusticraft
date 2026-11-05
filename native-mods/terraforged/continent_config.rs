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
    terraforged::lerp,
    terraforged_continent_cell::{CellShape, CellSource},
};

/// `ContinentConfig.CONTINENT_SAMPLE_SCALE`: the default continent cell size in blocks (`ShapeConfig::scale`).
pub const CONTINENT_SAMPLE_SCALE: i32 = 400;

/// TerraForged 1.18.2-0.3.1's `FloatRange` (`worldgen.noise.continent.config`): a mutable `min..max` pair.
/// `PartialEq` compares fields (the jar's class has no `equals`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FloatRange {
    pub min: f32,
    pub max: f32,
}

impl FloatRange {
    pub const fn new(min: f32, max: f32) -> Self {
        Self { min, max }
    }

    /// `FloatRange.at`: `NoiseUtil.lerp(min, max, alpha)`, unclamped.
    pub fn at(self, alpha: f32) -> f32 {
        lerp(self.min, self.max, alpha)
    }

    /// `FloatRange.copy`: takes `other`'s bounds and returns `self`, as the jar returns `this`.
    pub fn copy(&mut self, other: Self) -> &mut Self {
        self.min = other.min;
        self.max = other.max;
        self
    }

    /// `FloatRange.scale`: multiplies both bounds by `factor` in place.
    pub fn scale(&mut self, factor: f32) -> &mut Self {
        self.min *= factor;
        self.max *= factor;
        self
    }
}

/// The jar's `RiverConfig` (`worldgen.noise.continent.config`): carving widths and depths of the mod's own rivers or
/// lakes. Not Engine's `RiverConfig` (`terraforged_river`), which the mod's continent does not use. `Default` is
/// `new RiverConfig()`, the river preset.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RiverConfig {
    pub erosion: f32,
    pub bed_width: FloatRange,
    pub bank_width: FloatRange,
    pub valley_width: FloatRange,
    pub bed_depth: FloatRange,
    pub bank_depth: FloatRange,
}

impl RiverConfig {
    /// `RiverConfig.copy`: takes every field of `other` and returns `self`.
    pub fn copy(&mut self, other: Self) -> &mut Self {
        self.erosion = other.erosion;
        self.bed_depth.copy(other.bed_depth);
        self.bank_depth.copy(other.bank_depth);
        self.bed_width.copy(other.bed_width);
        self.bank_width.copy(other.bank_width);
        self.valley_width.copy(other.valley_width);
        self
    }

    /// `RiverConfig.scale`: scales the three widths only; erosion and the depths stay as they are.
    pub fn scale(&mut self, frequency: f32) -> &mut Self {
        self.bed_width.scale(frequency);
        self.bank_width.scale(frequency);
        self.valley_width.scale(frequency);
        self
    }

    /// `RiverConfig.lake()`: the river preset with a wider bed and banks, a narrower valley, a deeper bed and
    /// shallower banks (erosion unchanged).
    pub const fn lake() -> Self {
        Self {
            erosion: 0.075,
            bed_width: FloatRange::new(8.0, 15.0),
            bank_width: FloatRange::new(30.0, 45.0),
            valley_width: FloatRange::new(80.0, 120.0),
            bed_depth: FloatRange::new(2.0, 8.0),
            bank_depth: FloatRange::new(1.0, 1.5),
        }
    }
}

impl Default for RiverConfig {
    fn default() -> Self {
        Self {
            erosion: 0.075,
            bed_width: FloatRange::new(1.0, 7.0),
            bank_width: FloatRange::new(3.0, 30.0),
            valley_width: FloatRange::new(80.0, 200.0),
            bed_depth: FloatRange::new(1.25, 5.0),
            bank_depth: FloatRange::new(1.25, 3.0),
        }
    }
}

/// `ContinentConfig.Shape`. The jar's `ContinentNoise.createContinent` sets `scale` from Engine's `WorldSettings`
/// continent scale and draws `seed0`, then `seed1`, from the context `Seed`; `ContinentGenerator` takes `seed0` as
/// its cell seed and `seed1 + 6569` as its sample seed. The seeds default to 0 as in Java; `CellGenerator`
/// (`continent_cells.rs`) reads them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShapeConfig {
    pub seed0: i32,
    pub seed1: i32,
    pub scale: i32,
    pub jitter: f32,
    pub threshold: f32,
    pub base_falloff_min: f32,
    pub base_falloff_max: f32,
    pub cell_shape: CellShape,
    pub cell_source: CellSource,
}

impl Default for ShapeConfig {
    fn default() -> Self {
        Self {
            seed0: 0,
            seed1: 0,
            scale: CONTINENT_SAMPLE_SCALE,
            jitter: 0.75,
            threshold: 0.525,
            base_falloff_min: 0.01,
            base_falloff_max: 0.25,
            cell_shape: CellShape::Square,
            cell_source: CellSource::Perlin,
        }
    }
}

/// `ContinentConfig.Noise`: the falloff exponents of the continent shape's base and continent noise.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NoiseConfig {
    pub base_noise_falloff: f32,
    pub continent_noise_falloff: f32,
}

impl Default for NoiseConfig {
    fn default() -> Self {
        Self {
            base_noise_falloff: 1.5,
            continent_noise_falloff: 1.0,
        }
    }
}

/// `ContinentConfig.Rivers`: the river and lake presets and the lake density. The jar's river generator seeds from
/// `seed` and its carver from `seed + 21221`; nothing in the jar sets it, so it stays 0. Unused until the river
/// pipeline (#125) reads it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RiversConfig {
    pub seed: i32,
    pub lake_density: f32,
    pub rivers: RiverConfig,
    pub lakes: RiverConfig,
}

impl Default for RiversConfig {
    fn default() -> Self {
        Self {
            seed: 0,
            lake_density: 0.75,
            rivers: RiverConfig::default(),
            lakes: RiverConfig::lake(),
        }
    }
}

/// TerraForged 1.18.2-0.3.1's `ContinentConfig`: the settings of the mod's own continent noise (not Engine's
/// `WorldSettings`, whose continent scale and control points it reads elsewhere). A plain struct like the other
/// settings here: no codec or serializer is ported.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ContinentConfig {
    pub shape: ShapeConfig,
    pub noise: NoiseConfig,
    pub rivers: RiversConfig,
}

/// Bit-exact tests against `ContinentConfigOracle.java` (`.local/terraforged/oracles/continent_config/`), which runs the
/// jar's own config bytecode.
#[cfg(test)]
mod tests {
    use super::*;

    const MIN: f32 = f32::from_bits(1);
    const VALUES: [f32; 26] = [
        f32::NEG_INFINITY,
        -f32::MAX,
        -1e30,
        -16_777_217.0,
        -200.0,
        -7.25,
        -2.5,
        -1.0,
        -0.3,
        -MIN,
        -0.0,
        0.0,
        MIN,
        f32::MIN_POSITIVE,
        0.075,
        0.125,
        0.3,
        0.5,
        1.0,
        1.25,
        7.0,
        16_777_216.0,
        1e30,
        f32::MAX,
        f32::INFINITY,
        f32::NAN,
    ];

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

        fn range(&mut self, range: FloatRange) {
            self.add(range.min);
            self.add(range.max);
        }

        fn river(&mut self, config: RiverConfig) {
            self.add(config.erosion);
            self.range(config.bed_width);
            self.range(config.bank_width);
            self.range(config.valley_width);
            self.range(config.bed_depth);
            self.range(config.bank_depth);
        }

        fn check(&mut self, name: &str, count: usize, hash: u64) {
            assert_eq!((self.1, self.0), (count, hash), "{name}");
            *self = Self::new();
        }
    }

    fn range_bits(range: FloatRange) -> [u32; 2] {
        [range.min.to_bits(), range.max.to_bits()]
    }

    fn river_bits(config: RiverConfig) -> [u32; 11] {
        let [a, b] = range_bits(config.bed_width);
        let [c, d] = range_bits(config.bank_width);
        let [e, f] = range_bits(config.valley_width);
        let [g, h] = range_bits(config.bed_depth);
        let [i, j] = range_bits(config.bank_depth);
        [config.erosion.to_bits(), a, b, c, d, e, f, g, h, i, j]
    }

    const RIVER: [u32; 11] = [
        0x3d99_999a,
        0x3f80_0000,
        0x40e0_0000,
        0x4040_0000,
        0x41f0_0000,
        0x42a0_0000,
        0x4348_0000,
        0x3fa0_0000,
        0x40a0_0000,
        0x3fa0_0000,
        0x4040_0000,
    ];
    const LAKE: [u32; 11] = [
        0x3d99_999a,
        0x4100_0000,
        0x4170_0000,
        0x41f0_0000,
        0x4234_0000,
        0x42a0_0000,
        0x42f0_0000,
        0x4000_0000,
        0x4100_0000,
        0x3f80_0000,
        0x3fc0_0000,
    ];

    #[test]
    fn continent_config_defaults_match_jar() {
        let config = ContinentConfig::default();
        let shape = config.shape;
        assert_eq!(CONTINENT_SAMPLE_SCALE, 400);
        assert_eq!((shape.seed0, shape.seed1, shape.scale), (0, 0, 400));
        assert_eq!(
            [
                shape.jitter.to_bits(),
                shape.threshold.to_bits(),
                shape.base_falloff_min.to_bits(),
                shape.base_falloff_max.to_bits()
            ],
            [0x3f40_0000, 0x3f06_6666, 0x3c23_d70a, 0x3e80_0000]
        );
        assert_eq!(
            (shape.cell_shape, shape.cell_source),
            (CellShape::Square, CellSource::Perlin)
        );
        assert_eq!(
            [
                config.noise.base_noise_falloff.to_bits(),
                config.noise.continent_noise_falloff.to_bits()
            ],
            [0x3fc0_0000, 0x3f80_0000]
        );
        assert_eq!(
            (config.rivers.seed, config.rivers.lake_density.to_bits()),
            (0, 0x3f40_0000)
        );
        assert_eq!(river_bits(config.rivers.rivers), RIVER);
        assert_eq!(river_bits(config.rivers.lakes), LAKE);
        assert_eq!(river_bits(RiverConfig::default()), RIVER);
        assert_eq!(river_bits(RiverConfig::lake()), LAKE);
    }

    #[test]
    fn continent_config_ranges_match_jar() {
        let mut fnv = Fnv::new();
        for min in VALUES {
            for max in VALUES {
                for alpha in VALUES {
                    fnv.add(FloatRange::new(min, max).at(alpha));
                }
            }
        }
        fnv.check("at", 17_576, 0xa512_2753_4d24_8f3d);

        for min in VALUES {
            for max in VALUES {
                for factor in VALUES {
                    let mut range = FloatRange::new(min, max);
                    range.scale(factor);
                    fnv.range(range);
                    let mut copy = FloatRange::new(factor, -factor);
                    copy.copy(range);
                    fnv.range(copy);
                }
            }
        }
        fnv.check("range", 70_304, 0x6966_5fce_3dad_64ed);

        for factor in VALUES {
            let mut rivers = RiverConfig::default();
            rivers.copy(RiverConfig::lake()).scale(factor);
            fnv.river(rivers);
            let mut lakes = RiverConfig::lake();
            lakes.copy(RiverConfig::default()).scale(factor);
            fnv.river(lakes);
            let mut twice = RiverConfig::lake();
            twice.scale(factor).scale(factor);
            fnv.river(twice);
            let mut custom = RiverConfig::default();
            custom.erosion = factor;
            custom.bed_depth.min = factor;
            custom.bank_depth.max = -factor;
            let mut target = RiverConfig::lake();
            target.copy(custom);
            fnv.river(target);
        }
        fnv.check("river", 1_144, 0x14cb_68d6_897b_0560);
    }
}
