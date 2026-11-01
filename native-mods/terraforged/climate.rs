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

use super::terraforged::{Module, Source, SourceConfig, SourceKind, sin_lookup};

#[derive(Clone, Copy)]
pub struct ClimateRange {
    pub seed_offset: i32,
    pub scale: i32,
    pub falloff: i32,
    pub min: f32,
    pub max: f32,
    pub bias: f32,
}

impl ClimateRange {
    pub const fn new(scale: i32, falloff: i32, min: f32, max: f32, bias: f32) -> Self {
        Self {
            seed_offset: 0,
            scale,
            falloff,
            min,
            max,
            bias,
        }
    }

    pub fn min(self) -> f32 {
        self.min.min(self.max).clamp(0.0, 1.0)
    }

    pub fn max(self) -> f32 {
        self.min.max(self.max).clamp(self.min(), 1.0)
    }

    pub fn bias(self) -> f32 {
        self.bias.clamp(-1.0, 1.0)
    }

    pub fn apply(self, module: Module) -> Module {
        module
            .bias(Module::constant(self.bias() / 2.0))
            .clamp(Module::constant(self.min()), Module::constant(self.max()))
    }
}

impl Default for ClimateRange {
    fn default() -> Self {
        Self::new(7, 1, 0.0, 1.0, 0.0)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BiomeShape {
    pub biome_size: i32,
    pub macro_noise_size: i32,
    pub biome_warp_scale: i32,
    pub biome_warp_strength: i32,
}

impl BiomeShape {
    pub const DEFAULT_BIOME_SIZE: i32 = 225;
}

impl Default for BiomeShape {
    fn default() -> Self {
        Self {
            biome_size: Self::DEFAULT_BIOME_SIZE,
            macro_noise_size: 8,
            biome_warp_scale: 150,
            biome_warp_strength: 80,
        }
    }
}

#[derive(Clone, Copy)]
pub struct BiomeNoise {
    pub kind: SourceKind,
    pub scale: i32,
    pub octaves: i32,
    pub gain: f32,
    pub lacunarity: f32,
    pub strength: i32,
}

impl BiomeNoise {
    pub fn build(self, seed: i32) -> Module {
        Module::source(Source::new(
            self.kind,
            SourceConfig {
                seed,
                octaves: self.octaves as usize,
                gain: self.gain,
                frequency: 1.0 / self.scale as f32,
                lacunarity: self.lacunarity,
                ..SourceConfig::default()
            },
        ))
        .bias(Module::constant(-0.5))
    }
}

impl Default for BiomeNoise {
    fn default() -> Self {
        Self {
            kind: SourceKind::Simplex,
            scale: 24,
            octaves: 2,
            gain: 0.5,
            lacunarity: 2.65,
            strength: 14,
        }
    }
}

#[derive(Clone, Copy)]
pub struct ClimateSettings {
    pub temperature: ClimateRange,
    pub moisture: ClimateRange,
    pub biome_shape: BiomeShape,
    pub biome_edge_shape: BiomeNoise,
}

impl Default for ClimateSettings {
    fn default() -> Self {
        Self {
            temperature: ClimateRange::new(6, 2, 0.0, 0.98, 0.05),
            moisture: ClimateRange::new(6, 1, 0.0, 1.0, 0.0),
            biome_shape: BiomeShape::default(),
            biome_edge_shape: BiomeNoise::default(),
        }
    }
}

#[derive(Clone)]
pub struct Compressor {
    lower_start: f32,
    lower_end: f32,
    lower_range: f32,
    lower_expand_range: f32,
    upper_start: f32,
    upper_end: f32,
    upper_range: f32,
    upper_expanded_range: f32,
    compression: f32,
    compression_range: f32,
    module: Module,
}

impl Compressor {
    pub fn new(module: Module, inset: f32, amount: f32) -> Self {
        Self::with_ranges(
            module,
            inset,
            inset + amount,
            1.0 - inset - amount,
            1.0 - inset,
        )
    }

    pub fn with_ranges(
        module: Module,
        lower_start: f32,
        lower_end: f32,
        upper_start: f32,
        upper_end: f32,
    ) -> Self {
        Self {
            module,
            lower_start,
            lower_end,
            lower_range: lower_start,
            lower_expand_range: lower_end,
            upper_start,
            upper_end,
            upper_range: 1.0 - upper_end,
            upper_expanded_range: 1.0 - upper_start,
            compression: upper_start - lower_end,
            compression_range: upper_end - lower_start,
        }
    }

    pub fn value(&self, x: f32, y: f32) -> f32 {
        let value = self.module.value(x, y);
        if value <= self.lower_start {
            let alpha = value / self.lower_range;
            return alpha * self.lower_expand_range;
        }
        if value >= self.upper_end {
            let delta = value - self.upper_end;
            let alpha = delta / self.upper_range;
            return self.upper_start + alpha * self.upper_expanded_range;
        }
        let delta = value - self.lower_start;
        let alpha = delta / self.compression_range;
        self.lower_end + alpha * self.compression
    }

    pub fn module(self) -> Module {
        Module::custom(0.0, 1.0, move |x, y| self.value(x, y))
    }
}

#[derive(Clone, Copy)]
pub struct Temperature {
    power: i32,
    frequency: f32,
}

impl Temperature {
    pub const fn new(frequency: f32, power: i32) -> Self {
        Self { power, frequency }
    }

    pub fn value(self, x: f32, mut y: f32) -> f32 {
        let _ = x;
        y *= self.frequency;
        let sin = sin_lookup(y).clamp(-1.0, 1.0);
        let value = java_pow(sin, self.power as f32).copysign(sin);
        map_unit(value)
    }

    pub fn module(self) -> Module {
        Module::custom(0.0, 1.0, move |x, y| self.value(x, y))
    }
}

#[derive(Clone)]
pub struct Moisture {
    source: Module,
    power: i32,
}

impl Moisture {
    pub fn new(seed: i32, scale: i32, power: i32) -> Self {
        let source = Module::source(Source::new(
            SourceKind::Simplex,
            SourceConfig {
                seed,
                frequency: 1.0 / scale as f32,
                ..SourceConfig::default()
            },
        ))
        .clamp(Module::constant(0.125), Module::constant(0.875))
        .map(Module::constant(0.0), Module::constant(1.0));
        Self::from_module(source, power)
    }

    pub fn from_module(source: Module, power: i32) -> Self {
        Self {
            source: source.frequency(Module::constant(0.5), Module::constant(1.0)),
            power,
        }
    }

    pub fn value(&self, x: f32, y: f32) -> f32 {
        let mut noise = self.source.value(x, y);
        if self.power < 2 {
            return noise;
        }
        noise = (noise - 0.5) * 2.0;
        let value = java_pow(noise, self.power as f32).copysign(noise);
        map_unit(value)
    }

    pub fn module(self) -> Module {
        Module::custom(0.0, 1.0, move |x, y| self.value(x, y))
    }
}

fn java_pow(value: f32, power: f32) -> f32 {
    f64::from(value).powf(f64::from(power)) as f32
}

fn map_unit(value: f32) -> f32 {
    (value + 1.0) / 2.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bits(values: impl IntoIterator<Item = f32>) -> Vec<u32> {
        values.into_iter().map(f32::to_bits).collect()
    }

    #[test]
    fn climate_primitives_match_engine_0_3_0() {
        let x = Module::custom(f32::NEG_INFINITY, f32::INFINITY, |x, _| x);
        let compressor = Compressor::new(x.clone(), 0.2, 0.1);
        assert_eq!(
            bits(
                [-0.1, 0.0, 0.1, 0.2, 0.3, 0.5, 0.7, 0.8, 0.9, 1.0, 1.1]
                    .map(|value| compressor.value(value, 0.0))
            ),
            [
                0xbe19999a, 0x00000000, 0x3e19999a, 0x3e99999a, 0x3ebbbbbc, 0x3f000000, 0x3f222222,
                0x3f333333, 0x3f599999, 0x3f800000, 0x3f933334,
            ]
        );

        let temperature = Temperature::new(0.125, 3);
        assert_eq!(
            bits([-25.0, -10.0, -2.0, 0.0, 2.0, 10.0, 25.0].map(|y| { temperature.value(0.0, y) })),
            [
                0x3effffa4, 0x3d96b3dc, 0x3efc39b9, 0x3f000000, 0x3f01ebfb, 0x3f6d547b, 0x3f000023,
            ]
        );

        let moisture = Moisture::from_module(x, 3);
        assert_eq!(
            bits([0.0, 0.25, 0.5, 1.0, 1.5, 2.0].map(|x| moisture.value(x, 0.0))),
            [
                0x00000000, 0x3e940000, 0x3ee00000, 0x3f000000, 0x3f100000, 0x3f800000,
            ]
        );
    }

    #[test]
    fn climate_settings_match_engine_0_3_0() {
        let settings = ClimateSettings::default();
        assert_eq!(
            (
                settings.temperature.seed_offset,
                settings.temperature.scale,
                settings.temperature.falloff,
                settings.temperature.min.to_bits(),
                settings.temperature.max.to_bits(),
                settings.temperature.bias.to_bits(),
            ),
            (0, 6, 2, 0x00000000, 0x3f7ae148, 0x3d4ccccd)
        );
        assert_eq!(
            (
                settings.moisture.seed_offset,
                settings.moisture.scale,
                settings.moisture.falloff,
                settings.moisture.min.to_bits(),
                settings.moisture.max.to_bits(),
                settings.moisture.bias.to_bits(),
            ),
            (0, 6, 1, 0x00000000, 0x3f800000, 0x00000000)
        );
        assert_eq!(settings.biome_shape, BiomeShape::default());
    }

    #[test]
    fn climate_range_and_edge_noise_match_engine_0_3_0() {
        let range = ClimateRange {
            min: 0.8,
            max: 0.2,
            bias: 2.0,
            ..ClimateRange::default()
        };
        assert_eq!(
            bits([range.min(), range.max(), range.bias()]),
            [0x3e4ccccd, 0x3f4ccccd, 0x3f800000]
        );
        assert_eq!(
            range
                .apply(Module::constant(0.25))
                .value(0.0, 0.0)
                .to_bits(),
            0x3f400000
        );

        let edge = BiomeNoise::default().build(42);
        assert_eq!(
            bits(
                [
                    (0.0, 0.0),
                    (1.0, 2.0),
                    (24.0, 48.0),
                    (-10.0, 7.0),
                    (1000.0, -300.0)
                ]
                .map(|(x, y)| edge.value(x, y))
            ),
            [0x00000000, 0xbdd59d54, 0x3e314904, 0xbea34c20, 0xbe89fdf4]
        );
    }
}
