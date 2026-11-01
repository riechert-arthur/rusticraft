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
}
