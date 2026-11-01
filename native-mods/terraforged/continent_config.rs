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

use super::terraforged_continent_cell::{CellShape, CellSource};

pub const CONTINENT_SAMPLE_SCALE: i32 = 400;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FloatRange {
    pub min: f32,
    pub max: f32,
}

impl FloatRange {
    pub const fn new(min: f32, max: f32) -> Self {
        Self { min, max }
    }

    pub fn at(self, alpha: f32) -> f32 {
        self.min + alpha * (self.max - self.min)
    }

    pub fn copy(&mut self, other: Self) -> &mut Self {
        self.min = other.min;
        self.max = other.max;
        self
    }

    pub fn scale(&mut self, factor: f32) -> &mut Self {
        self.min *= factor;
        self.max *= factor;
        self
    }
}

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
    pub fn copy(&mut self, other: Self) -> &mut Self {
        self.erosion = other.erosion;
        self.bed_depth.copy(other.bed_depth);
        self.bank_depth.copy(other.bank_depth);
        self.bed_width.copy(other.bed_width);
        self.bank_width.copy(other.bank_width);
        self.valley_width.copy(other.valley_width);
        self
    }

    pub fn scale(&mut self, frequency: f32) -> &mut Self {
        self.bed_width.scale(frequency);
        self.bank_width.scale(frequency);
        self.valley_width.scale(frequency);
        self
    }

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

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ContinentConfig {
    pub shape: ShapeConfig,
    pub noise: NoiseConfig,
    pub rivers: RiversConfig,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bits(range: FloatRange) -> (u32, u32) {
        (range.min.to_bits(), range.max.to_bits())
    }

    #[test]
    fn continent_defaults_match_java() {
        let config = ContinentConfig::default();
        assert_eq!(
            (config.shape.seed0, config.shape.seed1, config.shape.scale),
            (0, 0, 400)
        );
        assert_eq!(config.shape.jitter.to_bits(), 0x3f40_0000);
        assert_eq!(config.shape.threshold.to_bits(), 0x3f06_6666);
        assert_eq!(config.shape.base_falloff_min.to_bits(), 0x3c23_d70a);
        assert_eq!(config.shape.base_falloff_max.to_bits(), 0x3e80_0000);
        assert_eq!(config.shape.cell_shape, CellShape::Square);
        assert_eq!(config.shape.cell_source, CellSource::Perlin);
        assert_eq!(config.noise.base_noise_falloff.to_bits(), 0x3fc0_0000);
        assert_eq!(config.noise.continent_noise_falloff.to_bits(), 0x3f80_0000);
        assert_eq!(config.rivers.lake_density.to_bits(), 0x3f40_0000);
        assert_eq!(config.rivers.rivers.erosion.to_bits(), 0x3d99_999a);
        assert_eq!(
            bits(config.rivers.rivers.bed_width),
            (0x3f80_0000, 0x40e0_0000)
        );
        assert_eq!(
            bits(config.rivers.rivers.bank_width),
            (0x4040_0000, 0x41f0_0000)
        );
        assert_eq!(
            bits(config.rivers.rivers.valley_width),
            (0x42a0_0000, 0x4348_0000)
        );
    }

    #[test]
    fn lake_and_range_mutations_match_java_raw_bits() {
        let lake = RiverConfig::lake();
        assert_eq!(bits(lake.bed_width), (0x4100_0000, 0x4170_0000));
        assert_eq!(bits(lake.bank_width), (0x41f0_0000, 0x4234_0000));
        assert_eq!(bits(lake.valley_width), (0x42a0_0000, 0x42f0_0000));
        assert_eq!(bits(lake.bed_depth), (0x4000_0000, 0x4100_0000));
        assert_eq!(bits(lake.bank_depth), (0x3f80_0000, 0x3fc0_0000));

        let mut range = FloatRange::new(-2.5, 7.25);
        assert_eq!(
            [
                range.at(-0.5).to_bits(),
                range.at(0.25).to_bits(),
                range.at(1.5).to_bits()
            ],
            [0xc0ec_0000, 0xbd80_0000, 0x4142_0000]
        );
        range.scale(0.3);
        assert_eq!(bits(range), (0xbf40_0000, 0x400b_3334));

        let mut copied = RiverConfig::default();
        copied.copy(lake).scale(0.125);
        assert_eq!(bits(copied.bed_width), (0x3f80_0000, 0x3ff0_0000));
        assert_eq!(bits(copied.bank_depth), (0x3f80_0000, 0x3fc0_0000));
    }
}
