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

use std::sync::{Arc, OnceLock};

#[derive(Clone, Copy)]
pub enum Interpolation {
    Linear,
    Curve3,
    Curve4,
}

impl Interpolation {
    pub fn apply(self, value: f32) -> f32 {
        match self {
            Self::Linear => value,
            Self::Curve3 => value * value * (3.0 - 2.0 * value),
            Self::Curve4 => value * value * value * (value * (value * 6.0 - 15.0) + 10.0),
        }
    }
}

const GRAD_2D: [[f32; 2]; 8] = [
    [-1.0, -1.0],
    [1.0, -1.0],
    [-1.0, 1.0],
    [1.0, 1.0],
    [0.0, -1.0],
    [-1.0, 0.0],
    [0.0, 1.0],
    [1.0, 0.0],
];

const GRAD_2D_24: [[f32; 2]; 32] = [
    [0.13052619, 0.9914449],
    [0.38268343, 0.9238795],
    [0.6087614, 0.7933533],
    [0.6087614, 0.7933533],
    [0.7933533, 0.6087614],
    [0.9238795, 0.38268343],
    [0.9914449, 0.13052619],
    [0.9914449, 0.13052619],
    [0.9914449, -0.13052619],
    [0.9238795, -0.38268343],
    [0.7933533, -0.6087614],
    [0.7933533, -0.6087614],
    [0.6087614, -0.7933533],
    [0.38268343, -0.9238795],
    [0.13052619, -0.9914449],
    [0.13052619, -0.9914449],
    [-0.13052619, -0.9914449],
    [-0.38268343, -0.9238795],
    [-0.6087614, -0.7933533],
    [-0.6087614, -0.7933533],
    [-0.7933533, -0.6087614],
    [-0.9238795, -0.38268343],
    [-0.9914449, -0.13052619],
    [-0.9914449, -0.13052619],
    [-0.9914449, 0.13052619],
    [-0.9238795, 0.38268343],
    [-0.7933533, 0.6087614],
    [-0.7933533, 0.6087614],
    [-0.6087614, 0.7933533],
    [-0.38268343, 0.9238795],
    [-0.13052619, 0.9914449],
    [-0.13052619, 0.9914449],
];

/// Java's `NoiseUtil.floor`. The `int` cast saturates and the `- 1` wraps, exactly as in Java.
pub fn floor(value: f32) -> i32 {
    if value >= 0.0 {
        value as i32
    } else {
        (value as i32).wrapping_sub(1)
    }
}

pub fn hash_2d(seed: i32, x: i32, y: i32) -> i32 {
    let mut hash = seed;
    hash ^= 1619_i32.wrapping_mul(x);
    hash ^= 31337_i32.wrapping_mul(y);
    hash = hash
        .wrapping_mul(hash)
        .wrapping_mul(hash)
        .wrapping_mul(60493);
    (hash >> 13) ^ hash
}

pub fn value_coord_2d(seed: i32, x: i32, y: i32) -> f32 {
    let mut value = seed;
    value ^= 1619_i32.wrapping_mul(x);
    value ^= 31337_i32.wrapping_mul(y);
    value
        .wrapping_mul(value)
        .wrapping_mul(value)
        .wrapping_mul(60493) as f32
        / 2_147_483_648.0_f32
}

fn gradient(seed: i32, x: i32, y: i32, better: bool) -> [f32; 2] {
    let hash = hash_2d(seed, x, y);
    if better {
        let selector = (((hash & 0x3f_ffff) as f32 * 1.3333334) as i32 & 31) as usize;
        GRAD_2D_24[selector]
    } else {
        GRAD_2D[(hash & 7) as usize]
    }
}

fn grad(seed: i32, x: i32, y: i32, xd: f32, yd: f32, better: bool) -> f32 {
    let gradient = gradient(seed, x, y, better);
    xd * gradient[0] + yd * gradient[1]
}

fn lerp(a: f32, b: f32, alpha: f32) -> f32 {
    a + alpha * (b - a)
}

/// Classic Perlin noise over the 8 directions of `GRAD_2D` (Java `Noise.singlePerlin`).
pub fn perlin(x: f32, y: f32, seed: i32, interpolation: Interpolation) -> f32 {
    perlin_with(x, y, seed, interpolation, false)
}

/// Perlin noise over the 24 directions of `GRAD_2D_24` (Java `Noise.singlePerlin2`).
pub fn perlin2(x: f32, y: f32, seed: i32, interpolation: Interpolation) -> f32 {
    perlin_with(x, y, seed, interpolation, true)
}

fn perlin_with(x: f32, y: f32, seed: i32, interpolation: Interpolation, better: bool) -> f32 {
    let x0 = floor(x);
    let y0 = floor(y);
    let x1 = x0.wrapping_add(1);
    let y1 = y0.wrapping_add(1);
    let xs = interpolation.apply(x - x0 as f32);
    let ys = interpolation.apply(y - y0 as f32);
    let xd0 = x - x0 as f32;
    let yd0 = y - y0 as f32;
    let xd1 = xd0 - 1.0;
    let yd1 = yd0 - 1.0;
    let lower = lerp(
        grad(seed, x0, y0, xd0, yd0, better),
        grad(seed, x1, y0, xd1, yd0, better),
        xs,
    );
    let upper = lerp(
        grad(seed, x0, y1, xd0, yd1, better),
        grad(seed, x1, y1, xd1, yd1, better),
        xs,
    );
    lerp(lower, upper, ys)
}

pub fn simplex(x: f32, y: f32, seed: i32) -> f32 {
    simplex_scaled(x, y, seed, 99.83685)
}

pub fn legacy_simplex(x: f32, y: f32, seed: i32) -> f32 {
    simplex_scaled(x, y, seed, 79.869484)
}

fn simplex_scaled(x: f32, y: f32, seed: i32, scale: f32) -> f32 {
    let skew = (x + y) * 0.36602542;
    let i = floor(x + skew);
    let j = floor(y + skew);
    let unskew = (i.wrapping_add(j)) as f32 * 0.21132487;
    let x0 = x - (i as f32 - unskew);
    let y0 = y - (j as f32 - unskew);
    let (i1, j1) = if x0 > y0 { (1, 0) } else { (0, 1) };
    let x1 = x0 - i1 as f32 + 0.21132487;
    let y1 = y0 - j1 as f32 + 0.21132487;
    let x2 = x0 - 1.0 + 0.42264974;
    let y2 = y0 - 1.0 + 0.42264974;
    fn corner(seed: i32, x: i32, y: i32, xd: f32, yd: f32) -> f32 {
        let mut attenuation = 0.5 - xd * xd - yd * yd;
        if attenuation < 0.0 {
            return 0.0;
        }
        attenuation *= attenuation;
        attenuation * attenuation * grad(seed, x, y, xd, yd, true)
    }
    scale
        * (corner(seed, i, j, x0, y0)
            + corner(seed, i.wrapping_add(i1), j.wrapping_add(j1), x1, y1)
            + corner(seed, i.wrapping_add(1), j.wrapping_add(1), x2, y2))
}

fn cubic_lerp(a: f32, b: f32, c: f32, d: f32, t: f32) -> f32 {
    let p = d - c - (a - b);
    t * t * t * p + t * t * (a - b - p) + t * (c - a) + b
}

pub fn cubic(x: f32, y: f32, seed: i32) -> f32 {
    let x1 = floor(x);
    let y1 = floor(y);
    let rows = [-1, 0, 1, 2].map(|dy| {
        let values =
            [-1, 0, 1, 2].map(|dx| value_coord_2d(seed, x1.wrapping_add(dx), y1.wrapping_add(dy)));
        cubic_lerp(values[0], values[1], values[2], values[3], x - x1 as f32)
    });
    cubic_lerp(rows[0], rows[1], rows[2], rows[3], y - y1 as f32) * 0.44444445
}

#[derive(Clone, Copy)]
pub struct SourceConfig {
    pub seed: i32,
    pub octaves: usize,
    pub gain: f32,
    pub frequency: f32,
    pub lacunarity: f32,
    pub interpolation: Interpolation,
}

impl Default for SourceConfig {
    fn default() -> Self {
        Self {
            seed: 1337,
            octaves: 1,
            gain: 0.5,
            frequency: 1.0,
            lacunarity: 2.0,
            interpolation: Interpolation::Curve3,
        }
    }
}

#[derive(Clone, Copy)]
pub enum SourceKind {
    Perlin,
    Perlin2,
    Simplex,
    Simplex2,
    Cubic,
    Ridge,
    Billow,
    SimplexRidge,
}

#[derive(Clone, Copy)]
pub struct Source {
    config: SourceConfig,
    kind: SourceKind,
    min: f32,
    max: f32,
    range: f32,
}

impl Source {
    pub fn new(kind: SourceKind, config: SourceConfig) -> Self {
        let (min, max) = match kind {
            SourceKind::Perlin | SourceKind::Perlin2 => {
                let bound = octave_bound(
                    &[1.0, 0.9, 0.83, 0.75, 0.64, 0.62, 0.61],
                    config,
                    config.gain,
                );
                (-bound, bound)
            }
            SourceKind::Simplex | SourceKind::Simplex2 => {
                let bound =
                    octave_bound(&[1.0, 0.989, 0.81, 0.781, 0.708, 0.702, 0.696], config, 1.0);
                (-bound, bound)
            }
            SourceKind::Cubic => {
                let bound = cubic_bound(0.75, config.octaves, config.gain);
                (-bound, bound)
            }
            SourceKind::Ridge | SourceKind::Billow => (0.0, ridge_bound(config)),
            SourceKind::SimplexRidge => {
                let bound =
                    octave_bound(&[1.0, 0.989, 0.81, 0.781, 0.708, 0.702, 0.696], config, 1.0);
                (0.0, bound)
            }
        };
        let range = max - min;
        Self {
            config,
            kind,
            min,
            max,
            // PerlinNoise, PerlinNoise2, RidgeNoise (so BillowNoise) and SimplexRidgeNoise take `Math.abs`:
            // their bound is negative for a negative gain (or, for the ridged ones, a negative lacunarity).
            range: match kind {
                SourceKind::Simplex | SourceKind::Simplex2 | SourceKind::Cubic => range,
                _ => range.abs(),
            },
        }
    }

    pub fn value(self, x: f32, y: f32) -> f32 {
        self.evaluate(x, y, self.config.seed, false)
    }

    pub fn value_seeded(self, x: f32, y: f32, seed: i32) -> f32 {
        self.evaluate(x, y, seed, true)
    }

    /// `Module.minValue()`: only `CubicNoise` overrides it, with its raw octave bound (not the mapped
    /// output range); the other sources keep `Noise`'s default of 0.
    pub fn min_value(self) -> f32 {
        match self.kind {
            SourceKind::Cubic => self.min,
            _ => 0.0,
        }
    }

    /// `Module.maxValue()`: the raw bound for `CubicNoise`, `Noise`'s default of 1 for the others.
    pub fn max_value(self) -> f32 {
        match self.kind {
            SourceKind::Cubic => self.max,
            _ => 1.0,
        }
    }

    fn evaluate(self, mut x: f32, mut y: f32, seed: i32, explicit_seed: bool) -> f32 {
        x *= self.config.frequency;
        y *= self.config.frequency;
        if matches!(self.kind, SourceKind::Cubic) {
            let mut sum = cubic(x, y, seed);
            let mut amplitude = 1.0;
            let mut current_seed = seed;
            for _ in 1..self.config.octaves {
                x *= self.config.lacunarity;
                y *= self.config.lacunarity;
                current_seed = current_seed.wrapping_add(1);
                amplitude *= self.config.gain;
                sum += cubic(x, y, current_seed) * amplitude;
            }
            return map(sum, self.min, self.max, self.range);
        }
        if matches!(
            self.kind,
            SourceKind::Ridge | SourceKind::Billow | SourceKind::SimplexRidge
        ) {
            let mut value = 0.0;
            let mut weight = 1.0;
            let mut amplitude = 2.0;
            let mut spectral_frequency = 1.0_f32;
            for octave in 0..self.config.octaves {
                let octave_seed = seed.wrapping_add(octave as i32);
                let mut signal = if matches!(self.kind, SourceKind::SimplexRidge) {
                    simplex(x, y, octave_seed)
                } else {
                    perlin(x, y, octave_seed, self.config.interpolation)
                };
                signal = 1.0 - signal.abs();
                signal *= signal;
                signal *= weight;
                weight = clamp(signal * amplitude, 0.0, 1.0);
                value += signal * spectral_weight(spectral_frequency);
                spectral_frequency *= self.config.lacunarity;
                x *= self.config.lacunarity;
                y *= self.config.lacunarity;
                amplitude *= self.config.gain;
            }
            let result = map(value, self.min, self.max, self.range);
            return if matches!(self.kind, SourceKind::Billow) {
                1.0 - result
            } else {
                result
            };
        }

        let mut sum = 0.0;
        let mut amplitude = if matches!(self.kind, SourceKind::Perlin | SourceKind::Perlin2) {
            self.config.gain
        } else {
            1.0
        };
        for octave in 0..self.config.octaves {
            let octave_seed = seed.wrapping_add(octave as i32);
            let signal = match self.kind {
                SourceKind::Perlin => perlin(x, y, octave_seed, self.config.interpolation),
                SourceKind::Perlin2 if !explicit_seed => {
                    perlin2(x, y, octave_seed, self.config.interpolation)
                }
                SourceKind::Perlin2 => perlin(x, y, octave_seed, self.config.interpolation),
                SourceKind::Simplex => legacy_simplex(x, y, octave_seed),
                SourceKind::Simplex2 => simplex(x, y, octave_seed),
                SourceKind::Cubic => unreachable!(),
                SourceKind::Ridge | SourceKind::Billow | SourceKind::SimplexRidge => unreachable!(),
            };
            sum += signal * amplitude;
            x *= self.config.lacunarity;
            y *= self.config.lacunarity;
            amplitude *= self.config.gain;
        }
        map(sum, self.min, self.max, self.range)
    }
}

fn octave_bound(signals: &[f32; 7], config: SourceConfig, mut amplitude: f32) -> f32 {
    let signal = signals[config.octaves.min(signals.len() - 1)];
    let mut sum = 0.0;
    for _ in 0..config.octaves {
        sum += signal * amplitude;
        amplitude *= config.gain;
    }
    sum
}

fn cubic_bound(signal: f32, octaves: usize, gain: f32) -> f32 {
    let mut amplitude = 1.0;
    let mut value = signal;
    for _ in 1..octaves {
        amplitude *= gain;
        value += signal * amplitude;
    }
    value
}

fn ridge_bound(config: SourceConfig) -> f32 {
    let mut value = 0.0;
    let mut weight = 1.0;
    let mut amplitude = 2.0;
    let mut spectral_frequency = 1.0_f32;
    for _ in 0..config.octaves {
        let mut noise = 1.0;
        noise *= noise;
        noise *= weight;
        weight = clamp(noise * amplitude, 0.0, 1.0);
        value += noise * spectral_weight(spectral_frequency);
        spectral_frequency *= config.lacunarity;
        amplitude *= config.gain;
    }
    value
}

/// `RidgeNoise.spectralWeights[octave]`: Java's `(float) Math.pow(frequency, -1.0)`, which is the correctly rounded
/// `1 / frequency` (checked over 24,030 lacunarity/octave pairs). Not `powf(-1.0)`: libm's `powf` differs in the last
/// bit for some inputs, and LLVM folds it to a division only in optimized builds. Java's table has 30 entries, so
/// Engine throws for more than 30 octaves; this port keeps going instead.
fn spectral_weight(frequency: f32) -> f32 {
    1.0 / frequency
}

fn clamp(value: f32, min: f32, max: f32) -> f32 {
    if value < min {
        min
    } else if value > max {
        max
    } else {
        value
    }
}

fn map(value: f32, min: f32, max: f32, range: f32) -> f32 {
    let clamped = clamp(value, min, max);
    let difference = clamped - min;
    if difference > range {
        1.0
    } else {
        difference / range
    }
}

/// Engine's `Rand` source: white noise at `frequency`, with `nextInt` kept exactly as Java computes it.
#[derive(Clone, Copy)]
pub struct Rand {
    seed: i32,
    frequency: f32,
}

impl Rand {
    pub fn new(seed: i32, frequency: f32) -> Self {
        Self { seed, frequency }
    }

    pub fn value(self, mut x: f32, mut y: f32) -> f32 {
        x *= self.frequency;
        y *= self.frequency;
        white(x, y, self.seed).abs()
    }

    pub fn value_seeded(self, x: f32, y: f32, child_seed: i32) -> f32 {
        white(x, y, hash(self.seed, child_seed))
    }

    pub fn next_int(self, x: f32, y: f32, range: i32) -> i32 {
        round(range as f32 * self.value(x, y) / range.wrapping_add(range) as f32)
    }

    pub fn next_int_seeded(self, x: f32, y: f32, child_seed: i32, range: i32) -> i32 {
        round(range as f32 * self.value_seeded(x, y, child_seed) / range.wrapping_add(range) as f32)
    }
}

fn white(x: f32, y: f32, seed: i32) -> f32 {
    value_coord_2d(seed, round(x), round(y))
}

fn round(value: f32) -> i32 {
    if value >= 0.0 {
        (value + 0.5) as i32
    } else {
        (value - 0.5) as i32
    }
}

fn hash(x: i32, y: i32) -> i32 {
    let mut hash = x ^ 31337_i32.wrapping_mul(y);
    hash = hash
        .wrapping_mul(hash)
        .wrapping_mul(hash)
        .wrapping_mul(60493);
    (hash >> 13) ^ hash
}

/// A node of Engine's `Module` graph. Cloning shares the node, as Java shares module instances between graphs.
/// Each node keeps the `minValue`/`maxValue` it reports, so samples never walk the graph to read bounds.
#[derive(Clone)]
pub struct Module(Arc<Node>);

struct Node {
    kind: ModuleKind,
    min: f32,
    max: f32,
}

/// A `MultiBlend` slot: its source and the `[min, max]` of the selector range where it is used alone.
struct BlendNode {
    source: Module,
    min: f32,
    max: f32,
}

enum ModuleKind {
    Constant(f32),
    Source(Source),
    Abs(Module),
    Alpha(Module, Module),
    Bias(Module, Module),
    Boost(Module, usize),
    Clamp(Module, Module, Module),
    Invert(Module),
    /// `Map` keeps `sourceRange = source.maxValue() - source.minValue()` from its constructor.
    Map(Module, Module, Module, f32),
    Power(Module, Module),
    Scale(Module, Module),
    Threshold(Module, Module),
    Warp(Module, Domain),
    /// `Base`: `min` is `base.maxValue()` and `max` is `min + falloff`, both fixed in the constructor.
    Base {
        base: Module,
        source: Module,
        min: f32,
        max: f32,
        falloff: f32,
        interpolation: Interpolation,
    },
    /// `Blend`: the blend window is fixed from the control's bounds in the constructor.
    Blend {
        control: Module,
        lower: Module,
        upper: Module,
        blend_lower: f32,
        blend_upper: f32,
        blend_range: f32,
        interpolation: Interpolation,
    },
    VariableBlend {
        control: Module,
        variable: Module,
        lower: Module,
        upper: Module,
        midpoint: f32,
        min_blend: f32,
        max_blend: f32,
        interpolation: Interpolation,
    },
    /// `MultiBlend`: the slots, `maxIndex` and `blendRange`, fixed in the constructor.
    MultiBlend {
        control: Module,
        nodes: Vec<BlendNode>,
        max_index: i32,
        blend_range: f32,
        interpolation: Interpolation,
    },
    /// `Select`: `lower_curve`/`upper_curve` hold Engine's `(curveMin, curveMax, curveRange)` per bound, fixed in the
    /// constructor.
    Select {
        control: Module,
        lower: Module,
        upper: Module,
        falloff: f32,
        lower_curve: (f32, f32, f32),
        upper_curve: (f32, f32, f32),
        interpolation: Interpolation,
    },
    Add(Vec<Module>),
    Multiply(Vec<Module>),
    Max(Vec<Module>),
}

impl Module {
    fn node(kind: ModuleKind, min: f32, max: f32) -> Self {
        Self(Arc::new(Node { kind, min, max }))
    }

    /// `Modifier`'s default bounds: those of its source.
    fn modifier(kind: ModuleKind, source: &Self) -> Self {
        Self::node(kind, source.min_value(), source.max_value())
    }

    pub fn constant(value: f32) -> Self {
        Self::node(ModuleKind::Constant(value), value, value)
    }

    pub fn source(source: Source) -> Self {
        Self::node(
            ModuleKind::Source(source),
            source.min_value(),
            source.max_value(),
        )
    }

    pub fn value(&self, x: f32, y: f32) -> f32 {
        match &self.0.kind {
            ModuleKind::Constant(value) => *value,
            ModuleKind::Source(source) => source.value(x, y),
            ModuleKind::Abs(source) => source.value(x, y).abs(),
            ModuleKind::Alpha(source, alpha) => {
                let value = source.value(x, y);
                let alpha = alpha.value(x, y);
                value * alpha + (1.0 - alpha)
            }
            ModuleKind::Bias(source, bias) => source.value(x, y) + bias.value(x, y),
            ModuleKind::Boost(source, iterations) => {
                let mut value = source.value(x, y);
                for _ in 0..*iterations {
                    value = java_pow(value, 1.0 - value);
                }
                value
            }
            ModuleKind::Clamp(source, min, max) => {
                let value = source.value(x, y);
                clamp(value, min.value(x, y), max.value(x, y))
            }
            ModuleKind::Invert(source) => {
                let value = source.value(x, y);
                if value > source.max_value() {
                    source.min_value()
                } else if value < source.min_value() {
                    source.max_value()
                } else {
                    source.max_value() - value
                }
            }
            ModuleKind::Map(source, min, max, range) => {
                let alpha = (source.value(x, y) - source.min_value()) / range;
                let min = min.value(x, y);
                let max = max.value(x, y);
                min + alpha * (max - min)
            }
            ModuleKind::Power(source, power) => {
                let value = source.value(x, y);
                java_pow(value, power.value(x, y))
            }
            ModuleKind::Scale(source, scale) => source.value(x, y) * scale.value(x, y),
            ModuleKind::Threshold(source, threshold) => {
                let value = source.value(x, y);
                if value < threshold.value(x, y) {
                    0.0
                } else {
                    1.0
                }
            }
            ModuleKind::Warp(source, domain) => source.value(domain.x(x, y), domain.y(x, y)),
            ModuleKind::Base {
                base,
                source,
                min,
                max,
                falloff,
                interpolation,
            } => {
                let upper = source.value(x, y);
                if upper < *max {
                    let lower = base.value(x, y);
                    if *falloff > 0.0 {
                        let alpha = (*max - java_max(*min, upper)) / *falloff;
                        lerp(upper, lower, interpolation.apply(alpha))
                    } else {
                        lower
                    }
                } else {
                    upper
                }
            }
            ModuleKind::Blend {
                control,
                lower,
                upper,
                blend_lower,
                blend_upper,
                blend_range,
                interpolation,
            } => {
                let select = control.value(x, y);
                if select < *blend_lower {
                    lower.value(x, y)
                } else if select > *blend_upper {
                    upper.value(x, y)
                } else {
                    let alpha = (select - *blend_lower) / *blend_range;
                    lerp(
                        lower.value(x, y),
                        upper.value(x, y),
                        interpolation.apply(alpha),
                    )
                }
            }
            ModuleKind::VariableBlend {
                control,
                variable,
                lower,
                upper,
                midpoint,
                min_blend,
                max_blend,
                interpolation,
            } => {
                let select = control.value(x, y);
                let radius = *min_blend + variable.value(x, y) * *max_blend;
                let min = java_max(0.0, *midpoint - radius);
                if select < min {
                    return lower.value(x, y);
                }
                let max = java_min(1.0, *midpoint + radius);
                if select > max {
                    return upper.value(x, y);
                }
                let alpha = (select - min) / (max - min);
                lerp(
                    lower.value(x, y),
                    upper.value(x, y),
                    interpolation.apply(alpha),
                )
            }
            ModuleKind::MultiBlend {
                control,
                nodes,
                max_index,
                blend_range,
                interpolation,
            } => {
                let selector = control.value(x, y);
                let index = round(selector * *max_index as f32);
                let mut min = multi_blend_slot(nodes, Some(index));
                let mut max = min;
                if *blend_range == 0.0 {
                    return min.source.value(x, y);
                }
                if selector > min.max {
                    max = multi_blend_slot(nodes, index.checked_add(1));
                } else if selector < min.min {
                    min = multi_blend_slot(nodes, index.checked_sub(1));
                } else {
                    return min.source.value(x, y);
                }
                let alpha = clamp((selector - min.max) / *blend_range, 0.0, 1.0);
                lerp(
                    min.source.value(x, y),
                    max.source.value(x, y),
                    interpolation.apply(alpha),
                )
            }
            ModuleKind::Select {
                control,
                lower,
                upper,
                falloff,
                lower_curve: (lower_min, lower_max, lower_range),
                upper_curve: (upper_min, upper_max, upper_range),
                interpolation,
            } => {
                let value = control.value(x, y);
                if *falloff == 0.0 {
                    // Engine's hard edge: only values above `upperBound` pick the upper source.
                    if value < *lower_max {
                        lower.value(x, y)
                    } else if value > *upper_min {
                        upper.value(x, y)
                    } else {
                        lower.value(x, y)
                    }
                } else if value < *lower_min {
                    lower.value(x, y)
                } else if value < *lower_max {
                    let alpha = (value - *lower_min) / *lower_range;
                    lerp(
                        lower.value(x, y),
                        upper.value(x, y),
                        interpolation.apply(alpha),
                    )
                } else if value < *upper_min {
                    upper.value(x, y)
                } else if value < *upper_max {
                    let alpha = (value - *upper_min) / *upper_range;
                    lerp(
                        upper.value(x, y),
                        lower.value(x, y),
                        interpolation.apply(alpha),
                    )
                } else {
                    lower.value(x, y)
                }
            }
            ModuleKind::Add(sources) => combine(sources, x, y, |a, b| a + b),
            ModuleKind::Multiply(sources) => combine(sources, x, y, |a, b| a * b),
            ModuleKind::Max(sources) => combine(sources, x, y, java_max),
        }
    }

    pub fn min_value(&self) -> f32 {
        self.0.min
    }

    pub fn max_value(&self) -> f32 {
        self.0.max
    }

    /// `Module.abs()`.
    pub fn abs(self) -> Self {
        if matches!(self.0.kind, ModuleKind::Abs(_)) {
            self
        } else {
            let source = self.clone();
            Self::modifier(ModuleKind::Abs(self), &source)
        }
    }

    /// `Module.alpha(Module)`: skipped when the alpha's bounds leave 0..1.
    pub fn alpha(self, alpha: Self) -> Self {
        if alpha.min_value() < 0.0 || alpha.max_value() > 1.0 {
            self
        } else {
            let source = self.clone();
            Self::modifier(ModuleKind::Alpha(self, alpha), &source)
        }
    }

    /// `Module.bias(Module)`: skipped for a zero bias.
    pub fn bias(self, bias: Self) -> Self {
        if bias.min_value() == 0.0 && bias.max_value() == 0.0 {
            self
        } else {
            let (min, max) = (
                self.min_value() + bias.min_value(),
                self.max_value() + bias.max_value(),
            );
            Self::node(ModuleKind::Bias(self, bias), min, max)
        }
    }

    /// `Module.boost(int)`: no-op for zero iterations; otherwise `Boost` maps its source to 0..1 first.
    pub fn boost(self, iterations: usize) -> Self {
        if iterations == 0 {
            self
        } else {
            let mapped = self.map(Self::constant(0.0), Self::constant(1.0));
            Self::modifier(ModuleKind::Boost(mapped.clone(), iterations), &mapped)
        }
    }

    /// `Module.clamp(Module, Module)`: skipped when constant bounds equal the source's.
    pub fn clamp(self, min: Self, max: Self) -> Self {
        if constant_bounds(&self, &min, &max) {
            self
        } else {
            let (low, high) = (min.min_value(), max.max_value());
            Self::node(ModuleKind::Clamp(self, min, max), low, high)
        }
    }

    /// `Module.invert()` (no shortcut in Engine: inverting twice nests).
    pub fn invert(self) -> Self {
        let source = self.clone();
        Self::modifier(ModuleKind::Invert(self), &source)
    }

    /// `Module.map(Module, Module)`: skipped when constant bounds equal the source's.
    pub fn map(self, min: Self, max: Self) -> Self {
        if constant_bounds(&self, &min, &max) {
            self
        } else {
            let range = self.max_value() - self.min_value();
            let (low, high) = (min.min_value(), max.max_value());
            Self::node(ModuleKind::Map(self, min, max, range), low, high)
        }
    }

    /// `Module.pow(Module)`: a constant 0 power gives `Source.ONE`, a constant 1 power the source itself.
    pub fn power(self, power: Self) -> Self {
        if power.min_value() == 0.0 && power.max_value() == 0.0 {
            Self::constant(1.0)
        } else if power.min_value() == 1.0 && power.max_value() == 1.0 {
            self
        } else {
            let source = self.clone();
            Self::modifier(ModuleKind::Power(self, power), &source)
        }
    }

    /// `Module.scale(Module)`: skipped for a scale of 1; bounds are fixed when it is built.
    pub fn scale(self, scale: Self) -> Self {
        if scale.min_value() == 1.0 && scale.max_value() == 1.0 {
            return self;
        }
        let min = self.min_value() * scale.min_value();
        let max = self.max_value() * scale.max_value();
        Self::node(ModuleKind::Scale(self, scale), min, max)
    }

    /// `Module.threshold(Module)`: Engine reports the source's bounds, not 0..1.
    pub fn threshold(self, threshold: Self) -> Self {
        let source = self.clone();
        Self::modifier(ModuleKind::Threshold(self, threshold), &source)
    }

    /// `Module.warp(Domain)`: samples the source at the warped point; reports the source's bounds.
    pub fn warp(self, domain: Domain) -> Self {
        let source = self.clone();
        Self::modifier(ModuleKind::Warp(self, domain), &source)
    }

    /// `Module.base(Module, falloff, Interpolation)` (`new Base(this, other, ...)`; Engine's two-argument form
    /// uses `Interpolation.CURVE3`). Reports the base's minimum and the larger of both maxima.
    pub fn base(self, source: Self, falloff: f32, interpolation: Interpolation) -> Self {
        let min = self.max_value();
        let max = self.max_value() + falloff;
        let (low, high) = (
            self.min_value(),
            java_max(self.max_value(), source.max_value()),
        );
        let kind = ModuleKind::Base {
            base: self,
            source,
            min,
            max,
            falloff,
            interpolation,
        };
        Self::node(kind, low, high)
    }

    /// `Module.blend(Module, Module, midpoint, blendRange, Interpolation)` (Engine's four-argument form uses
    /// `Interpolation.LINEAR`). Reports the smaller minimum and larger maximum of `lower` and `upper`.
    pub fn blend(
        self,
        lower: Self,
        upper: Self,
        midpoint: f32,
        blend_range: f32,
        interpolation: Interpolation,
    ) -> Self {
        let mid = self.min_value() + (self.max_value() - self.min_value()) * midpoint;
        let blend_lower = java_max(self.min_value(), mid - blend_range / 2.0);
        let blend_upper = java_min(self.max_value(), mid + blend_range / 2.0);
        let (low, high) = selector_bounds(&lower, &upper);
        let kind = ModuleKind::Blend {
            control: self,
            lower,
            upper,
            blend_lower,
            blend_upper,
            blend_range: blend_upper - blend_lower,
            interpolation,
        };
        Self::node(kind, low, high)
    }

    /// `Module.blendVar(variable, Module, Module, midpoint, min, max, Interpolation)` (`VariableBlend`).
    pub fn variable_blend(
        self,
        variable: Self,
        lower: Self,
        upper: Self,
        midpoint: f32,
        min_blend: f32,
        max_blend: f32,
        interpolation: Interpolation,
    ) -> Self {
        let (low, high) = selector_bounds(&lower, &upper);
        let kind = ModuleKind::VariableBlend {
            control: self,
            variable,
            lower,
            upper,
            midpoint,
            min_blend,
            max_blend,
            interpolation,
        };
        Self::node(kind, low, high)
    }

    /// `Module.blend(blend, Interpolation, sources...)` (`MultiBlend`; Engine's two-argument form uses
    /// `Interpolation.LINEAR`). Splits the control's 0..1 range into one evenly spaced slot per source and blends
    /// neighbouring slots. Reports the smallest minimum and largest maximum of the sources (0 and 0 when empty).
    pub fn multi_blend(self, blend: f32, interpolation: Interpolation, sources: Vec<Self>) -> Self {
        let spacing = 1.0 / sources.len() as f32;
        let radius = spacing / 2.0;
        let blend_range = radius * blend;
        let cell_radius = (radius - blend_range) / 2.0;
        // Java's `int maxIndex = sources.length - 1`: -1 without sources.
        let max_index = sources.len() as i32 - 1;
        let low = fold_bounds(&sources, Self::min_value, |a, b| java_min(a, b.min_value()));
        let high = fold_bounds(&sources, Self::max_value, |a, b| java_max(a, b.max_value()));
        let nodes = sources
            .into_iter()
            .enumerate()
            .map(|(index, source)| {
                let position = index as f32 * spacing + radius;
                let min = if index == 0 {
                    0.0
                } else {
                    position - cell_radius
                };
                let max = if index as i32 == max_index {
                    1.0
                } else {
                    position + cell_radius
                };
                BlendNode {
                    source,
                    min: java_max(0.0, min),
                    max: java_min(1.0, max),
                }
            })
            .collect();
        let kind = ModuleKind::MultiBlend {
            control: self,
            nodes,
            max_index,
            blend_range,
            interpolation,
        };
        Self::node(kind, low, high)
    }

    /// `Module.select(Module, Module, lowerBound, upperBound, falloff, Interpolation)` (Engine's five-argument
    /// form uses `Interpolation.CURVE3`).
    pub fn select(
        self,
        lower: Self,
        upper: Self,
        lower_bound: f32,
        upper_bound: f32,
        falloff: f32,
        interpolation: Interpolation,
    ) -> Self {
        let curve = |bound: f32| {
            let (min, max) = (bound - falloff, bound + falloff);
            (min, max, max - min)
        };
        let (low, high) = selector_bounds(&lower, &upper);
        let kind = ModuleKind::Select {
            control: self,
            lower,
            upper,
            falloff,
            lower_curve: curve(lower_bound),
            upper_curve: curve(upper_bound),
            interpolation,
        };
        Self::node(kind, low, high)
    }

    /// `new Add(sources...)`.
    pub fn add(sources: Vec<Self>) -> Self {
        let min = fold_bounds(&sources, Self::min_value, |a, b| a + b.min_value());
        let max = fold_bounds(&sources, Self::max_value, |a, b| a + b.max_value());
        Self::node(ModuleKind::Add(sources), min, max)
    }

    /// `new Multiply(sources...)`.
    pub fn multiply(sources: Vec<Self>) -> Self {
        let min = fold_bounds(&sources, Self::min_value, |a, b| a * b.min_value());
        let max = fold_bounds(&sources, Self::max_value, |a, b| a * b.max_value());
        Self::node(ModuleKind::Multiply(sources), min, max)
    }

    /// `new Max(sources...)`: Engine's `minTotal` is `maxTotal`, so the minimum folds the later maxima.
    pub fn max(sources: Vec<Self>) -> Self {
        let min = fold_bounds(&sources, Self::min_value, |a, b| java_max(a, b.max_value()));
        let max = fold_bounds(&sources, Self::max_value, |a, b| java_max(a, b.max_value()));
        Self::node(ModuleKind::Max(sources), min, max)
    }
}

/// Engine's `Domain`: a coordinate warp. Cloning shares the node, as Java shares domain instances.
#[derive(Clone)]
pub struct Domain(Arc<DomainKind>);

enum DomainKind {
    /// `Domain.DIRECT`.
    Direct,
    /// `DomainWarp`: both axis modules are already mapped to -0.5..0.5.
    DomainWarp {
        x: Module,
        y: Module,
        distance: Module,
    },
    /// `DirectionWarp`.
    Direction { direction: Module, strength: Module },
    /// `AddWarp`.
    Add(Domain, Domain),
    /// `CompoundWarp`.
    Compound(Domain, Domain),
    /// `CumulativeWarp`.
    Cumulative(Domain, Domain),
}

impl Domain {
    fn node(kind: DomainKind) -> Self {
        Self(Arc::new(kind))
    }

    /// `Domain.DIRECT`.
    pub fn direct() -> Self {
        Self::node(DomainKind::Direct)
    }

    /// `Domain.warp(Module, Module, Module)` (`new DomainWarp`): each axis is mapped to -0.5..0.5 unless it already
    /// reports exactly those bounds (`Module.map`'s own shortcut is the same test).
    pub fn domain_warp(x: Module, y: Module, distance: Module) -> Self {
        let map = |axis: Module| axis.map(Module::constant(-0.5), Module::constant(0.5));
        Self::node(DomainKind::DomainWarp {
            x: map(x),
            y: map(y),
            distance,
        })
    }

    /// `Domain.direction(Module, Module)` (`new DirectionWarp`).
    pub fn direction(direction: Module, strength: Module) -> Self {
        Self::node(DomainKind::Direction {
            direction,
            strength,
        })
    }

    /// `Domain.add(Domain)` (`AddWarp`): the two offsets summed.
    #[allow(clippy::should_implement_trait)]
    pub fn add(self, next: Self) -> Self {
        Self::node(DomainKind::Add(self, next))
    }

    /// `Domain.warp(Domain)` (`CompoundWarp`): `next`'s offset at the point warped by `self`.
    pub fn compound(self, next: Self) -> Self {
        Self::node(DomainKind::Compound(self, next))
    }

    /// `Domain.then(Domain)` (`CumulativeWarp`): Engine returns `next`'s warped *coordinate* as the offset, so
    /// `x()` adds the input coordinate once more.
    pub fn then(self, next: Self) -> Self {
        Self::node(DomainKind::Cumulative(self, next))
    }

    /// `Domain.getOffsetX`.
    pub fn offset_x(&self, x: f32, y: f32) -> f32 {
        match self.0.as_ref() {
            DomainKind::Direct => 0.0,
            DomainKind::DomainWarp {
                x: axis, distance, ..
            } => axis.value(x, y) * distance.value(x, y),
            DomainKind::Direction {
                direction,
                strength,
            } => {
                let angle = direction.value(x, y) * PI2;
                sin(angle) * strength.value(x, y)
            }
            DomainKind::Add(first, second) => first.offset_x(x, y) + second.offset_x(x, y),
            DomainKind::Compound(first, second) => second.offset_x(first.x(x, y), first.y(x, y)),
            DomainKind::Cumulative(first, second) => second.x(first.x(x, y), first.y(x, y)),
        }
    }

    /// `Domain.getOffsetY`.
    pub fn offset_y(&self, x: f32, y: f32) -> f32 {
        match self.0.as_ref() {
            DomainKind::Direct => 0.0,
            DomainKind::DomainWarp {
                y: axis, distance, ..
            } => axis.value(x, y) * distance.value(x, y),
            DomainKind::Direction {
                direction,
                strength,
            } => {
                let angle = direction.value(x, y) * PI2;
                cos(angle) * strength.value(x, y)
            }
            DomainKind::Add(first, second) => first.offset_y(x, y) + second.offset_y(x, y),
            DomainKind::Compound(first, second) => second.offset_y(first.x(x, y), first.y(x, y)),
            DomainKind::Cumulative(first, second) => second.y(first.x(x, y), first.y(x, y)),
        }
    }

    /// `Domain.getX`.
    pub fn x(&self, x: f32, y: f32) -> f32 {
        x + self.offset_x(x, y)
    }

    /// `Domain.getY`.
    pub fn y(&self, x: f32, y: f32) -> f32 {
        y + self.offset_y(x, y)
    }
}

/// The shortcut test shared by `Module.clamp` and `Module.map`.
fn constant_bounds(source: &Module, min: &Module, max: &Module) -> bool {
    min.min_value() == min.max_value()
        && min.min_value() == source.min_value()
        && max.min_value() == max.max_value()
        && max.max_value() == source.max_value()
}

fn combine(sources: &[Module], x: f32, y: f32, operation: impl Fn(f32, f32) -> f32) -> f32 {
    let Some((first, rest)) = sources.split_first() else {
        return 0.0;
    };
    rest.iter().fold(first.value(x, y), |value, source| {
        operation(value, source.value(x, y))
    })
}

/// `MultiBlend`'s `nodes[index]`. Engine throws `ArrayIndexOutOfBoundsException` when the control leaves 0..1 far
/// enough to pick a slot that does not exist (or has no sources); this port panics there instead of inventing a value.
fn multi_blend_slot(nodes: &[BlendNode], index: Option<i32>) -> &BlendNode {
    index
        .and_then(|index| usize::try_from(index).ok())
        .and_then(|index| nodes.get(index))
        .expect("MultiBlend control selects a slot outside its sources, where Engine throws")
}

/// `Selector`'s `Combiner` bounds over `{lower, upper}`: `Math.min` of the minima, `Math.max` of the maxima.
fn selector_bounds(lower: &Module, upper: &Module) -> (f32, f32) {
    (
        java_min(lower.min_value(), upper.min_value()),
        java_max(lower.max_value(), upper.max_value()),
    )
}

/// `Combiner`'s constructor: the first source's bound, then `minTotal`/`maxTotal` over the rest; 0 when empty.
fn fold_bounds(
    sources: &[Module],
    first: impl Fn(&Module) -> f32,
    total: impl Fn(f32, &Module) -> f32,
) -> f32 {
    let Some((head, rest)) = sources.split_first() else {
        return 0.0;
    };
    rest.iter().fold(first(head), total)
}

fn java_pow(value: f32, power: f32) -> f32 {
    (f64::from(value).powf(f64::from(power))) as f32
}

fn java_max(first: f32, second: f32) -> f32 {
    if first.is_nan() {
        first
    } else if second.is_nan() {
        second
    } else if first == 0.0 && second == 0.0 {
        if first.is_sign_positive() || second.is_sign_positive() {
            0.0
        } else {
            -0.0
        }
    } else if first >= second {
        first
    } else {
        second
    }
}

fn java_min(first: f32, second: f32) -> f32 {
    if first.is_nan() {
        first
    } else if second.is_nan() {
        second
    } else if first == 0.0 && second == 0.0 {
        if first.is_sign_negative() || second.is_sign_negative() {
            -0.0
        } else {
            0.0
        }
    } else if first <= second {
        first
    } else {
        second
    }
}

/// `NoiseUtil.PI2` and `radFull`: `(float) Math.PI * 2` (the cast binds first; doubling is exact either way).
const PI2: f32 = std::f64::consts::PI as f32 * 2.0;
const SIN_MASK: i32 = 4095;
const SIN_COUNT: usize = 4096;

/// `NoiseUtil.SIN`, built once in the order of Engine's static initializer.
fn sin_table() -> &'static [f32; SIN_COUNT] {
    static TABLE: OnceLock<[f32; SIN_COUNT]> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut table = [0.0; SIN_COUNT];
        for (i, entry) in table.iter_mut().enumerate() {
            let angle = (i as f32 + 0.5) / SIN_COUNT as f32 * PI2;
            *entry = f64::from(angle).sin() as f32;
        }
        let deg_to_index = SIN_COUNT as f32 / 360.0;
        for degrees in (0..360).step_by(90) {
            let index = ((degrees as f32 * deg_to_index) as i32 & SIN_MASK) as usize;
            table[index] = (f64::from(degrees) * std::f64::consts::PI / 180.0).sin() as f32;
        }
        table
    })
}

/// `NoiseUtil.sin`: a 4096-entry lookup (`as i32` saturates and maps NaN to 0, like Java's `(int)`).
fn sin(radians: f32) -> f32 {
    let rad_to_index = SIN_COUNT as f32 / PI2;
    sin_table()[((radians * rad_to_index) as i32 & SIN_MASK) as usize]
}

/// `NoiseUtil.cos`: `sin(r + 1.5708f)`.
fn cos(radians: f32) -> f32 {
    sin(radians + f32::from_bits(0x3fc9_0ff9))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bits(values: impl IntoIterator<Item = f32>) -> Vec<u32> {
        values.into_iter().map(f32::to_bits).collect()
    }

    /// Like `bits`, but every NaN becomes 0x7fc00000: the sign and payload of a NaN depend on the CPU.
    fn canonical_bits(values: impl IntoIterator<Item = f32>) -> Vec<u32> {
        values
            .into_iter()
            .map(|v| if v.is_nan() { 0x7fc0_0000 } else { v.to_bits() })
            .collect()
    }

    #[test]
    fn extreme_coordinates_wrap_like_engine_0_3_0() {
        // Java ints wrap where Rust's debug builds would panic. Expected values come from Engine 0.3.0.
        let points = [
            (-3.0e9, 3.0e9),
            (3.0e9, -3.0e9),
            (f32::MIN, f32::MAX),
            (f32::MAX, f32::MIN),
            (-2.0, -2.0),
        ];
        assert_eq!(
            points.map(|(x, _)| floor(x)),
            [i32::MAX, i32::MAX, i32::MAX, i32::MAX, -3]
        );
        const NAN: u32 = 0x7fc0_0000;
        assert_eq!(
            canonical_bits(points.map(|(x, y)| perlin(x, y, 918273, Interpolation::Curve3))),
            [0xff800000, 0x7f800000, 0xff800000, NAN, 0]
        );
        assert_eq!(
            canonical_bits(points.map(|(x, y)| perlin2(x, y, 918273, Interpolation::Curve4))),
            [NAN, 0xff800000, NAN, NAN, 0]
        );
        assert_eq!(
            canonical_bits(points.map(|(x, y)| simplex(x, y, 918273))),
            [0, 0, 0, 0, 0xbe92101e]
        );
        assert_eq!(
            canonical_bits(points.map(|(x, y)| legacy_simplex(x, y, 918273))),
            [0, 0, 0, 0, 0xbe69b364]
        );
        assert_eq!(
            canonical_bits(points.map(|(x, y)| cubic(x, y, 918273))),
            [NAN, 0x7f800000, NAN, NAN, 0xbeb9b92b]
        );
    }

    #[test]
    fn grid_sweep_matches_engine_0_3_0() {
        // Every source and interpolation over a 48x48 grid of negative and positive points for four seeds, folded
        // into one FNV-1a style checksum of the raw bits. Engine 0.3.0 runs the same loop and prints 0x79cb944a.
        let mut acc: u32 = 0x811c_9dc5;
        let mut mix = |v: u32| acc = (acc ^ v).wrapping_mul(0x0100_0193);
        let mut count = 0;
        for seed in [0, -1, 918273, i32::MIN] {
            for iy in -24..24 {
                for ix in -24..24 {
                    let x = ix as f32 * 0.37 - 0.05;
                    let y = iy as f32 * 0.29 + 0.11;
                    for i in [
                        Interpolation::Linear,
                        Interpolation::Curve3,
                        Interpolation::Curve4,
                    ] {
                        mix(perlin(x, y, seed, i).to_bits());
                        mix(perlin2(x, y, seed, i).to_bits());
                    }
                    mix(simplex(x, y, seed).to_bits());
                    mix(legacy_simplex(x, y, seed).to_bits());
                    mix(cubic(x, y, seed).to_bits());
                    mix(hash_2d(seed, ix, iy) as u32);
                    mix(value_coord_2d(seed, ix, iy).to_bits());
                    mix(floor(x) as u32);
                    count += 1;
                }
            }
        }
        assert_eq!((count, acc), (9216, 0x79cb_944a));
    }

    #[test]
    fn hashing_matches_engine_0_3_0() {
        let inputs = [(0, 0, 0), (42, -17, 91), (i32::MIN, 1_234_567, -7_654_321)];
        assert_eq!(
            inputs.map(|(s, x, y)| hash_2d(s, x, y) as u32),
            [0, 0x40612059, 0x0572ac7c]
        );
        assert_eq!(
            bits(inputs.map(|(s, x, y)| value_coord_2d(s, x, y))),
            [0, 0x3f00c646, 0x3d2e50fd]
        );
    }

    #[test]
    fn sources_match_engine_0_3_0() {
        assert_eq!(Interpolation::Linear.apply(0.25).to_bits(), 0x3e800000);
        let points = [
            (0.0, 0.0),
            (0.125, -0.75),
            (-17.25, 91.5),
            (12345.678, -9876.543),
        ];
        assert_eq!(
            bits(points.map(|(x, y)| perlin(x, y, 918273, Interpolation::Curve3))),
            [0x80000000, 0x3e6ad000, 0, 0x3eead8e4]
        );
        assert_eq!(
            bits(points.map(|(x, y)| perlin2(x, y, 918273, Interpolation::Curve4))),
            [0, 0x3d84e722, 0xbdf8df90, 0xbed422fc]
        );
        assert_eq!(
            bits(points.map(|(x, y)| simplex(x, y, 918273))),
            [0, 0xbf0c83ca, 0x3d2f52b0, 0xbf2e3484]
        );
        assert_eq!(
            bits(points.map(|(x, y)| legacy_simplex(x, y, 918273))),
            [0, 0xbee0d2dd, 0x3d0c4227, 0xbf0b5d37]
        );
        assert_eq!(
            bits(points.map(|(x, y)| cubic(x, y, 918273))),
            [0xbea370a8, 0x3ea40cd4, 0xbe4d83e2, 0x3d2b86ab]
        );
    }

    #[test]
    fn octave_sources_match_engine_0_3_0() {
        let config = SourceConfig {
            seed: 8128,
            octaves: 5,
            gain: 0.47,
            frequency: 0.0078125,
            lacunarity: 2.13,
            interpolation: Interpolation::Curve4,
        };
        let points = [
            (0.0, 0.0),
            (12.25, -98.75),
            (-4096.5, 8192.25),
            (123456.75, -0.125),
        ];
        let cases = [
            (
                SourceKind::Perlin,
                [0x3f000000, 0x3ecb61fa, 0x3eb19ea9, 0x3f352d0f],
                [0x3f000000, 0x3ebc4868, 0x3ef9e565, 0x3ea831a9],
            ),
            (
                SourceKind::Perlin2,
                [0x3f000000, 0x3f00ddf5, 0x3edd96ab, 0x3f186b18],
                [0x3f000000, 0x3ebc4868, 0x3ef9e565, 0x3ea831a9],
            ),
            (
                SourceKind::Simplex,
                [0x3f000000, 0x3f14e4f8, 0x3f168604, 0x3f5d81ab],
                [0x3f000000, 0x3f351311, 0x3f12ee34, 0x3f3d7a61],
            ),
            (
                SourceKind::Simplex2,
                [0x3f000000, 0x3f1a1e36, 0x3f1c2785, 0x3f74e215],
                [0x3f000000, 0x3f4257d5, 0x3f17a9c1, 0x3f4cd8fa],
            ),
            (
                SourceKind::Cubic,
                [0x3f097eca, 0x3f1ca60a, 0x3ef5738d, 0x3f19b1d4],
                [0x3f0bac93, 0x3f04f827, 0x3ee8f0cb, 0x3ef94bc2],
            ),
        ];
        for (kind, expected, expected_seeded) in cases {
            let source = Source::new(kind, config);
            assert_eq!(bits(points.map(|(x, y)| source.value(x, y))), expected);
            assert_eq!(
                bits(points.map(|(x, y)| source.value_seeded(x, y, -991))),
                expected_seeded
            );
        }
    }

    #[test]
    fn source_bounds_and_edge_configs_match_engine_0_3_0() {
        // Negative gain (PerlinNoise's `Math.abs` range), more octaves than the signal table, a seed that
        // wraps per octave, zero octaves (0/0 = NaN after `map`) and a plain two-octave config.
        let configs = [
            (2147483646, 9, -0.6, 0.031, 1.9, Interpolation::Curve4),
            (-7, 0, 0.5, 0.25, 2.0, Interpolation::Linear),
            (1337, 2, 0.5, 1.0, 2.0, Interpolation::Curve3),
        ];
        let kinds = [
            SourceKind::Perlin,
            SourceKind::Perlin2,
            SourceKind::Simplex,
            SourceKind::Simplex2,
            SourceKind::Cubic,
        ];
        let points = [(0.0, 0.0), (3.5, -17.25), (-1000.125, 250.75)];
        // Per config and kind: minValue, maxValue, getValue(x, y) and getValue(x, y, Integer.MAX_VALUE).
        let expected: [[(u32, u32, [u32; 3], [u32; 3]); 5]; 3] = [
            [
                (0, 0x3f800000, [0; 3], [0; 3]),
                (0, 0x3f800000, [0; 3], [0; 3]),
                (
                    0,
                    0x3f800000,
                    [0x3f000000, 0, 0],
                    [0x3f000000, 0, 0x3ded045d],
                ),
                (
                    0,
                    0x3f800000,
                    [0x3f000000, 0, 0],
                    [0x3f000000, 0, 0x3ca115dd],
                ),
                (
                    0xbef26b2d,
                    0x3ef26b2d,
                    [0x3ebd193a, 0x3ed8de12, 0x3e98209c],
                    [0x3f35b51d, 0x3f3d793c, 0],
                ),
            ],
            [
                (0, 0x3f800000, [NAN; 3], [NAN; 3]),
                (0, 0x3f800000, [NAN; 3], [NAN; 3]),
                (0, 0x3f800000, [NAN; 3], [NAN; 3]),
                (0, 0x3f800000, [NAN; 3], [NAN; 3]),
                (
                    0xbf400000,
                    0x3f400000,
                    [0x3efe88c4, 0x3f032de8, 0x3ed472b4],
                    [0x3f4bd987, 0x3ee2e739, 0x3e8f95ed],
                ),
            ],
            [
                (
                    0,
                    0x3f800000,
                    [0x3f000000, 0x3f2e2f4b, 0x3f085555],
                    [0x3f000000, 0x3f01346f, 0x3f192829],
                ),
                (
                    0,
                    0x3f800000,
                    [0x3f000000, 0x3eec18aa, 0x3f06c11b],
                    [0x3f000000, 0x3f01346f, 0x3f192829],
                ),
                (
                    0,
                    0x3f800000,
                    [0x3f000000, 0x3efc1cbf, 0x3f415051],
                    [0x3f000000, 0x3eef7b45, 0x3e10910a],
                ),
                (
                    0,
                    0x3f800000,
                    [0x3f000000, 0x3efb23ee, 0x3f51a465],
                    [0x3f000000, 0x3eeb5a17, 0x3d52d533],
                ),
                (
                    0xbf900000,
                    0x3f900000,
                    [0x3ecc2ffc, 0x3f162025, 0x3f1fb20c],
                    [0x3f329104, 0x3ee08b97, 0x3f2547da],
                ),
            ],
        ];
        // Any NaN matches NAN: Java printed x86's default 0xffc00000, which rustc may constant-fold differently.
        const NAN: u32 = 0x7fc00000;
        let canon = |v: f32| if v.is_nan() { NAN } else { v.to_bits() };
        for ((seed, octaves, gain, frequency, lacunarity, interpolation), rows) in
            configs.into_iter().zip(expected)
        {
            let config = SourceConfig {
                seed,
                octaves,
                gain,
                frequency,
                lacunarity,
                interpolation,
            };
            for (kind, (min, max, plain, seeded)) in kinds.into_iter().zip(rows) {
                let source = Source::new(kind, config);
                assert_eq!(source.min_value().to_bits(), min);
                assert_eq!(source.max_value().to_bits(), max);
                assert_eq!(points.map(|(x, y)| canon(source.value(x, y))), plain);
                assert_eq!(
                    points.map(|(x, y)| canon(source.value_seeded(x, y, i32::MAX))),
                    seeded
                );
            }
        }
    }

    #[test]
    fn ridged_and_random_sources_match_engine_0_3_0() {
        let config = SourceConfig {
            seed: -7391,
            octaves: 6,
            gain: 0.83,
            frequency: 0.00390625,
            lacunarity: 2.25,
            interpolation: Interpolation::Curve3,
        };
        let points = [
            (0.0, 0.0),
            (12.25, -98.75),
            (-4096.5, 8192.25),
            (123456.75, -0.125),
        ];
        let cases = [
            (
                SourceKind::Ridge,
                [0x3f800000, 0x3f4e3bad, 0x3f6e7958, 0x3f0119da],
                [0x3f800000, 0x3f58fe28, 0x3f784536, 0x3f51f9ce],
            ),
            (
                SourceKind::Billow,
                [0, 0x3e47114c, 0x3d8c3540, 0x3efdcc4c],
                [0, 0x3e1c0760, 0x3cf75940, 0x3e3818c8],
            ),
            (
                SourceKind::SimplexRidge,
                [0x3f25da6e, 0x3e8542ea, 0x3ddba237, 0x3e609c6f],
                [0x3f25da6e, 0x3e7e4e32, 0x3e116da0, 0x3df253c5],
            ),
        ];
        for (kind, expected, expected_seeded) in cases {
            let source = Source::new(kind, config);
            assert_eq!(bits(points.map(|(x, y)| source.value(x, y))), expected);
            assert_eq!(
                bits(points.map(|(x, y)| source.value_seeded(x, y, 991))),
                expected_seeded
            );
        }

        let rand = Rand::new(5519, 0.125);
        assert_eq!(
            bits(points.map(|(x, y)| rand.value(x, y))),
            [0x3f0322bd, 0x3f1d629d, 0x3ee51611, 0x3e86b835]
        );
        assert_eq!(
            bits(points.map(|(x, y)| rand.value_seeded(x, y, -17))),
            [0x3e4239b0, 0x3f64c89d, 0xbe335bca, 0x3f33d648]
        );
        assert_eq!(points.map(|(x, y)| rand.next_int(x, y, 19)), [0; 4]);
        assert_eq!(
            points.map(|(x, y)| rand.next_int_seeded(x, y, -17, 19)),
            [0; 4]
        );
    }

    #[test]
    fn ridged_bounds_and_edge_configs_match_engine_0_3_0() {
        // Negative gain, zero octaves (0/0 = NaN), a negative lacunarity (negative spectral weights), Java's maximum
        // of 30 octaves, and a negative bound (gain -2, lacunarity -0.5), where `map` needs the `Math.abs` range:
        // without it every value would be 1. Expected values come from Engine 0.3.0.
        let configs = [
            (2147483646, 9, -0.6, 0.031, 1.9, Interpolation::Curve4),
            (-7, 0, 0.5, 0.25, 2.0, Interpolation::Linear),
            (1337, 2, 0.5, 1.0, 2.0, Interpolation::Curve3),
            (99, 5, 0.9, 0.07, -1.7, Interpolation::Curve3),
            (-123456, 30, 0.975, 0.013, 1.3, Interpolation::Curve4),
            (55, 2, -2.0, 0.05, -0.5, Interpolation::Curve3),
        ];
        let kinds = [
            SourceKind::Ridge,
            SourceKind::Billow,
            SourceKind::SimplexRidge,
        ];
        let points = [(0.0, 0.0), (3.5, -17.25), (-1000.125, 250.75)];
        const NAN: u32 = 0x7fc0_0000;
        // Per config and kind: getValue(x, y) and getValue(x, y, Integer.MAX_VALUE). minValue/maxValue are 0/1.
        let expected: [[([u32; 3], [u32; 3]); 3]; 6] = [
            [
                (
                    [0x3f800000, 0x3f4d2122, 0x3f520b5f],
                    [0x3f800000, 0x3ecdaf4c, 0x3f37390b],
                ),
                ([0, 0x3e4b7b78, 0x3e37d284], [0, 0x3f19285a, 0x3e918dea]),
                (
                    [0x3f800000, 0x3d71889a, 0x3f800000],
                    [0x3f800000, 0x3e001264, 0x3f32c6be],
                ),
            ],
            [([NAN; 3], [NAN; 3]); 3],
            [
                (
                    [0x3f800000, 0x3ecf1800, 0x3ef1416b],
                    [0x3f800000, 0x3f7c0600, 0x3f3b1a38],
                ),
                ([0, 0x3f187400, 0x3f075f4a], [0, 0x3c7e8000, 0x3e89cb90]),
                (
                    [0x3f800000, 0x3f0ce83e, 0x3e06d15c],
                    [0x3f800000, 0x3f43ba6f, 0x3d1403b4],
                ),
            ],
            [
                (
                    [0x3f800000, 0x3f5c9df8, 0x3cc72c78],
                    [0x3f800000, 0x3ef517b8, 0x3f4bcfcd],
                ),
                ([0, 0x3e0d8820, 0x3f79c69c], [0, 0x3f057424, 0x3e50c0cc]),
                (
                    [0x3e701273, 0x3e1968a2, 0x3d8e7ff2],
                    [0x3e701273, 0x3bdaf8e8, 0x3d417929],
                ),
            ],
            [
                (
                    [0x3f800000, 0x3f257ac0, 0x3f106165],
                    [0x3f800000, 0x3ef0c36f, 0x3f089c10],
                ),
                ([0, 0x3eb50a80, 0x3edf3d36], [0, 0x3f079e48, 0x3eeec7e0]),
                (
                    [0x3e95b5b5, 0x3c9d537b, 0x3ce43e05],
                    [0x3e95b5b5, 0x3a6fe6b4, 0x3bce989d],
                ),
            ],
            [
                ([0; 3], [0; 3]),
                ([0x3f800000; 3], [0x3f800000; 3]),
                ([0; 3], [0, 0xbf800000, 0xbf800000]),
            ],
        ];
        for ((seed, octaves, gain, frequency, lacunarity, interpolation), rows) in
            configs.into_iter().zip(expected)
        {
            let config = SourceConfig {
                seed,
                octaves,
                gain,
                frequency,
                lacunarity,
                interpolation,
            };
            for (kind, (plain, seeded)) in kinds.into_iter().zip(rows) {
                let source = Source::new(kind, config);
                assert_eq!(source.min_value(), 0.0);
                assert_eq!(source.max_value(), 1.0);
                assert_eq!(
                    canonical_bits(points.map(|(x, y)| source.value(x, y))),
                    plain
                );
                assert_eq!(
                    canonical_bits(points.map(|(x, y)| source.value_seeded(x, y, i32::MAX))),
                    seeded
                );
            }
        }
    }

    #[test]
    fn ridged_sweep_matches_engine_0_3_0() {
        // FNV-1a style checksums of raw bits; Engine 0.3.0 runs the same loops.
        let mut acc: u32 = 0x811c_9dc5;
        let mut count = 0;
        for i in -400..=400 {
            let lacunarity = i as f32 * 0.0137 + 0.001;
            let mut frequency = 1.0_f32;
            for _ in 0..30 {
                acc = (acc ^ spectral_weight(frequency).to_bits()).wrapping_mul(0x0100_0193);
                frequency *= lacunarity;
                count += 1;
            }
        }
        assert_eq!((count, acc), (24030, 0x9153df22));

        let mut acc: u32 = 0x811c_9dc5;
        let mut mix = |v: f32| acc = (acc ^ v.to_bits()).wrapping_mul(0x0100_0193);
        let mut count = 0;
        for lacunarity in [1.5, 2.0, 2.25, 3.1, 0.7] {
            for gain in [0.5, 0.83, 1.1, -0.4] {
                let config = SourceConfig {
                    seed: 4242,
                    octaves: 7,
                    gain,
                    frequency: 0.011,
                    lacunarity,
                    interpolation: Interpolation::Curve3,
                };
                let ridge = Source::new(SourceKind::Ridge, config);
                let billow = Source::new(SourceKind::Billow, config);
                let simplex_ridge = Source::new(SourceKind::SimplexRidge, config);
                for iy in -10..10 {
                    for ix in -10..10 {
                        let x = ix as f32 * 37.3 - 5.1;
                        let y = iy as f32 * 29.7 + 1.3;
                        mix(ridge.value(x, y));
                        mix(billow.value_seeded(x, y, -5));
                        mix(simplex_ridge.value(x, y));
                        count += 1;
                    }
                }
            }
        }
        assert_eq!((count, acc), (8000, 0x7d958e37));
    }

    #[test]
    fn rand_next_int_matches_engine_0_3_0() {
        // `range * noise / (range + range)` rounds to 0 for small ranges; `Integer.MAX_VALUE + Integer.MAX_VALUE`
        // wraps to -2 in Java, so the largest range gives large negative values.
        let rand = Rand::new(5519, 0.125);
        let points = [
            (0.0, 0.0),
            (12.25, -98.75),
            (-4096.5, 8192.25),
            (123456.75, -0.125),
            (-3.5, 2.5),
        ];
        for range in [0, 1, 19, 1000, -5] {
            assert_eq!(points.map(|(x, y)| rand.next_int(x, y, range)), [0; 5]);
            assert_eq!(
                points.map(|(x, y)| rand.next_int_seeded(x, y, -17, range)),
                [0; 5]
            );
        }
        assert_eq!(
            points.map(|(x, y)| rand.next_int(x, y, i32::MAX)),
            [-550022976, -660121408, -480428576, -282527392, -550022976]
        );
        assert_eq!(
            points.map(|(x, y)| rand.next_int_seeded(x, y, -17, i32::MAX)),
            [-203660032, -959588160, 188071072, -754291200, -894462208]
        );
        assert_eq!(
            bits(points.map(|(x, y)| rand.value_seeded(x, y, i32::MAX))),
            [0xbe1a0663, 0x3baa8000, 0xbceece10, 0x3f529ef7, 0xbf5f7d26]
        );
    }

    #[test]
    fn module_graph_matches_engine_0_3_0() {
        let base = Module::source(Source::new(
            SourceKind::Simplex,
            SourceConfig {
                seed: 8128,
                octaves: 4,
                gain: 0.5,
                frequency: 1.0 / 128.0,
                lacunarity: 2.0,
                interpolation: Interpolation::Curve3,
            },
        ));
        let shaped = base
            .clone()
            .bias(Module::constant(0.125))
            .scale(Module::constant(0.75))
            .clamp(Module::constant(0.2), Module::constant(0.8))
            .map(Module::constant(-1.0), Module::constant(2.0))
            .abs()
            .invert();
        let boosted = base
            .clone()
            .alpha(Module::constant(0.4))
            .boost(2)
            .power(Module::constant(1.75));
        let gated = boosted.clone().threshold(Module::constant(0.55));
        let points = [
            (0.0, 0.0),
            (12.25, -98.75),
            (-4096.5, 8192.25),
            (123456.75, -0.125),
        ];
        let cases = [
            (
                shaped.clone(),
                [0xbf800000, 0x40000000],
                [0x3fd40000, 0x3fa11d66, 0x3f5b6d74, 0x3e8b2530],
            ),
            (
                boosted,
                [0, 0x3f800000],
                [0x3f7f20fa, 0x3f7fad65, 0x3f7fe9b7, 0x3f7fff19],
            ),
            (gated, [0, 0x3f800000], [0x3f800000; 4]),
            (
                Module::add(vec![base.clone(), shaped.clone()]),
                [0xbf800000, 0x40400000],
                [0x400a0000, 0x3feeaf28, 0x3fc8fd77, 0x3f920b16],
            ),
            (
                Module::multiply(vec![base.clone(), shaped.clone()]),
                [0x80000000, 0x40000000],
                [0x3f540000, 0x3f43464f, 0x3f1c7914, 0x3e71e388],
            ),
            (
                Module::max(vec![base, shaped]),
                [0x40000000, 0x40000000],
                [0x3fd40000, 0x3fa11d66, 0x3f5b6d74, 0x3f5e8394],
            ),
        ];
        for (module, expected_bounds, expected_values) in cases {
            assert_eq!(
                [module.min_value().to_bits(), module.max_value().to_bits()],
                expected_bounds
            );
            assert_eq!(
                bits(points.map(|(x, y)| module.value(x, y))),
                expected_values
            );
        }
        let max_bounds = Module::max(vec![
            Module::source(Source::new(SourceKind::Simplex, SourceConfig::default())),
            Module::constant(0.5),
        ]);
        assert_eq!(
            [
                max_bounds.min_value().to_bits(),
                max_bounds.max_value().to_bits()
            ],
            [0x3f000000, 0x3f800000]
        );
    }

    #[test]
    fn module_graph_reads_cubic_bounds_like_engine_0_3_0() {
        // CubicNoise is the only source whose bounds are not Noise's 0/1, so the modifiers that read their source's
        // bounds (Map, Boost's map, Invert, Clamp's and Map's shortcuts, Scale, the combiners) see its raw octave bound.
        // Also covers Module.map's and Module.pow's shortcuts and nested abs/invert. Expected values come from Engine 0.3.0.
        let cubic = Source::new(
            SourceKind::Cubic,
            SourceConfig {
                seed: 31,
                octaves: 3,
                gain: 0.5,
                frequency: 1.0 / 64.0,
                lacunarity: 2.0,
                interpolation: Interpolation::Curve3,
            },
        );
        let simplex = Module::source(Source::new(
            SourceKind::Simplex,
            SourceConfig {
                seed: 8128,
                octaves: 4,
                gain: 0.5,
                frequency: 1.0 / 128.0,
                lacunarity: 2.0,
                interpolation: Interpolation::Curve3,
            },
        ));
        let (low, high) = (cubic.min_value(), cubic.max_value());
        let cubic = Module::source(cubic);
        let c = Module::constant;
        let points = [
            (0.0, 0.0),
            (12.25, -98.75),
            (-4096.5, 8192.25),
            (123456.75, -0.125),
            (-3.5, 2.5),
        ];
        const RAW: [u32; 5] = [0x3f2dacf3, 0x3eb80f89, 0x3ec8031e, 0x3eefe935, 0x3f2a2ed2];
        const CUBIC: [u32; 2] = [0xbfa80000, 0x3fa80000];
        let cases = [
            (cubic.clone(), CUBIC, RAW),
            (
                cubic.clone().map(c(-1.0), c(2.0)),
                [0xbf800000, 0x40000000],
                [0x3fa33e44, 0x3f692d72, 0x3f724aee, 0x3f848bc6, 0x3fa13f54],
            ),
            (cubic.clone().map(c(low), c(high)), CUBIC, RAW),
            (
                cubic.clone().boost(2),
                [0, 0x3f800000],
                [0x3f7ee5d8, 0x3f79bec4, 0x3f7a93a0, 0x3f7c48eb, 0x3f7ecb49],
            ),
            (
                cubic.clone().invert(),
                CUBIC,
                [0x3f22530d, 0x3f73f83c, 0x3f6bfe71, 0x3f580b66, 0x3f25d12e],
            ),
            (
                cubic.clone().clamp(c(0.1), c(0.4)),
                [0x3dcccccd, 0x3ecccccd],
                [0x3ecccccd, 0x3eb80f89, 0x3ec8031e, 0x3ecccccd, 0x3ecccccd],
            ),
            (cubic.clone().clamp(c(low), c(high)), CUBIC, RAW),
            (
                cubic.clone().power(c(0.0)),
                [0x3f800000, 0x3f800000],
                [0x3f800000; 5],
            ),
            (cubic.clone().power(c(1.0)), CUBIC, RAW),
            (
                cubic.clone().power(c(1.5)),
                CUBIC,
                [0x3f0f0cd3, 0x3e5cb7a7, 0x3e7a05d8, 0x3ea439b0, 0x3f0ac1b5],
            ),
            (
                cubic
                    .clone()
                    .scale(c(0.5))
                    .bias(c(0.25))
                    .abs()
                    .threshold(c(0.3)),
                [0xbed00000, 0x3f680000],
                [0x3f800000; 5],
            ),
            (
                cubic.clone().alpha(c(0.4)),
                CUBIC,
                [0x3f5f11fc, 0x3f3e6982, 0x3f419a3a, 0x3f49950b, 0x3f5dac54],
            ),
            // Engine's `invert()` has no shortcut, so inverting twice does not give the source back.
            (
                cubic.clone().invert().invert(),
                CUBIC,
                [0x3f2dacf3, 0x3eb80f88, 0x3ec8031e, 0x3eefe934, 0x3f2a2ed2],
            ),
            (
                cubic.clone().bias(c(-1.0)).abs().abs(),
                [0xc0140000, 0x3ea00000],
                [0x3ea4a61a, 0x3f23f83c, 0x3f1bfe71, 0x3f080b66, 0x3eaba25c],
            ),
            (
                Module::add(vec![cubic.clone(), simplex.clone()]),
                [0xbfa80000, 0x40140000],
                [0x3f96d67a, 0x3f772b4a, 0x3f8d4784, 0x3fab3c17, 0x3f885a72],
            ),
            (
                Module::multiply(vec![cubic.clone(), simplex.clone()]),
                [0x80000000, 0x3fa80000],
                [0x3eadacf3, 0x3e5f15e3, 0x3e8ea0c0, 0x3ed0878b, 0x3e884f90],
            ),
            (
                Module::max(vec![cubic.clone(), simplex.clone()]),
                [0x3f800000, 0x3fa80000],
                [0x3f2dacf3, 0x3f1b2385, 0x3f368d7a, 0x3f5e8394, 0x3f2a2ed2],
            ),
            (
                Module::max(vec![simplex.clone(), cubic]),
                [0x3fa80000, 0x3fa80000],
                [0x3f2dacf3, 0x3f1b2385, 0x3f368d7a, 0x3f5e8394, 0x3f2a2ed2],
            ),
            (
                simplex.boost(1),
                [0, 0x3f800000],
                [0x3f3504f3, 0x3f5227a8, 0x3f6854cd, 0x3f7b5934, 0x3f13e7a9],
            ),
        ];
        for (index, (module, expected_bounds, expected_values)) in cases.into_iter().enumerate() {
            assert_eq!(
                [module.min_value().to_bits(), module.max_value().to_bits()],
                expected_bounds,
                "case {index}"
            );
            assert_eq!(
                bits(points.map(|(x, y)| module.value(x, y))),
                expected_values,
                "case {index}"
            );
        }
    }

    #[test]
    fn domain_warps_match_engine_0_3_0() {
        let source = |kind, seed, scale: f32, octaves, gain| {
            Module::source(Source::new(
                kind,
                SourceConfig {
                    seed,
                    octaves,
                    gain,
                    frequency: 1.0 / scale,
                    lacunarity: 2.0,
                    interpolation: Interpolation::Curve3,
                },
            ))
        };
        let warp = Domain::domain_warp(
            source(SourceKind::Perlin, 110, 64.0, 3, 0.5),
            source(SourceKind::Simplex, 220, 96.0, 2, 0.5),
            Module::constant(37.5),
        );
        let direction = Domain::direction(
            source(SourceKind::Simplex, 330, 72.0, 3, 0.5),
            Module::constant(11.25),
        );
        let points = [
            (0.0, 0.0),
            (12.25, -98.75),
            (-4096.5, 8192.25),
            (123456.75, -0.125),
        ];
        let cases = [
            (
                Domain::direct(),
                [
                    [0, 0, 0, 0],
                    [0, 0, 0x41440000, 0xc2c58000],
                    [0, 0, 0xc5800400, 0x46000100],
                    [0, 0, 0x47f12060, 0xbe000000],
                ],
            ),
            (
                warp.clone(),
                [
                    [0, 0, 0, 0],
                    [0xc0bec34d, 0x4156e18b, 0x40c93cb3, 0xc2aaa3cf],
                    [0x3df4d364, 0xbf334ce1, 0xc580030b, 0x45fffc66],
                    [0xbd01b190, 0x41257e96, 0x47f1205c, 0x41237e96],
                ],
            ),
            (
                direction.clone(),
                [
                    [0x26c68d2e, 0xc1340000, 0x26c68d2e, 0xc1340000],
                    [0xc12463b5, 0xc092a4cb, 0x3ffce258, 0xc2ceaa4d],
                    [0xc04b2f4b, 0xc12caf18, 0xc5801d66, 0x45ffaba8],
                    [0xc132b251, 0xbfacfb79, 0x47f11aca, 0xbfbcfb79],
                ],
            ),
            (
                warp.clone().add(direction.clone()),
                [
                    [0x26c68d2e, 0xc1340000, 0x26c68d2e, 0xc1340000],
                    [0xc181e2ae, 0x410d8f26, 0xc07f1570, 0xc2b3ce1b],
                    [0xc04388b0, 0xc137e3e6, 0xc5801c71, 0x45ffa60e],
                    [0xc1333403, 0x410fdf27, 0x47f11ac6, 0x410ddf27],
                ],
            ),
            (
                warp.clone().compound(direction.clone()),
                [
                    [0x26c68d2e, 0xc1340000, 0x26c68d2e, 0xc1340000],
                    [0x3f212ad7, 0xc133b7ca, 0x414e12ad, 0xc2dbf6f9],
                    [0xc07f83a3, 0xc12848b9, 0xc58023f0, 0x45ffaddc],
                    [0xc10cf689, 0xc0dfdeb2, 0x47f11bf8, 0xc0e3deb2],
                ],
            ),
            (
                warp.clone().then(direction.clone()),
                [
                    [0x26c68d2e, 0xc1340000, 0x26c68d2e, 0xc1340000],
                    [0x40dd620e, 0xc2c11ac8, 0x41995884, 0xc3434d64],
                    [0xc58022fb, 0x45ffa842, 0xc600137e, 0x467fd521],
                    [0x47f11bf4, 0x404e3cf4, 0x48711e2a, 0x40463cf4],
                ],
            ),
            (
                // Axes already reporting -0.5..0.5 are not mapped again; a constant axis maps to NaN, as in Engine.
                Domain::domain_warp(
                    source(SourceKind::Perlin, 110, 64.0, 3, 0.5)
                        .map(Module::constant(-0.5), Module::constant(0.5)),
                    Module::constant(0.5).map(Module::constant(-0.5), Module::constant(0.5)),
                    Module::constant(4.0),
                ),
                [
                    [0, 0xffc00000, 0, 0xffc00000],
                    [0xbf22c8ca, 0xffc00000, 0x4139d373, 0xffc00000],
                    [0x3c50eb00, 0xffc00000, 0xc58003e6, 0xffc00000],
                    [0xbb5d5800, 0xffc00000, 0x47f12060, 0xffc00000],
                ],
            ),
            (
                warp.clone()
                    .then(direction.clone())
                    .add(warp.clone().compound(direction.clone()))
                    .then(warp.clone()),
                [
                    [0xc1037ef0, 0xc1336565, 0xc1037ef0, 0xc1336565],
                    [0x41d1b9ba, 0xc34504d1, 0x4219dcdd, 0xc393e268],
                    [0xc6002703, 0x467fb917, 0xc6402903, 0x46bfdd0c],
                    [0x48711d62, 0xc149cc10, 0x48b4d6c9, 0xc14bcc10],
                ],
            ),
        ];
        for (domain, expected) in cases {
            assert_eq!(
                points.map(|(x, y)| [
                    domain.offset_x(x, y).to_bits(),
                    domain.offset_y(x, y).to_bits(),
                    domain.x(x, y).to_bits(),
                    domain.y(x, y).to_bits(),
                ]),
                expected
            );
        }
        let warped = source(SourceKind::Ridge, 440, 128.0, 4, 0.975).warp(warp.add(direction));
        assert_eq!(
            [warped.min_value().to_bits(), warped.max_value().to_bits()],
            [0, 0x3f800000]
        );
        assert_eq!(
            bits(points.map(|(x, y)| warped.value(x, y))),
            [0x3f4fea9c, 0x3f2d9330, 0x3f4b5329, 0x3f53bd2e]
        );
    }

    #[test]
    fn sin_lookup_matches_engine_0_3_0() {
        // Java: `hash = hash * 31 + floatToRawIntBits(SIN[i])` over `NoiseUtil.SIN`.
        let hash = sin_table().iter().fold(0_i64, |hash, value| {
            hash.wrapping_mul(31)
                .wrapping_add(i64::from(value.to_bits() as i32))
        });
        assert_eq!(hash, 4517474909631308587);
        let angles = [
            0.0,
            1e-7,
            -1e-7,
            f32::from_bits(0x3fc9_0ff9),
            -f32::from_bits(0x3fc9_0ff9),
            std::f32::consts::PI,
            -std::f32::consts::PI,
            PI2,
            -PI2,
            100.25,
            -100.25,
            1e9,
            -1e9,
            f32::NAN,
            f32::INFINITY,
            f32::NEG_INFINITY,
            std::f32::consts::FRAC_PI_4,
            -2.3561945,
        ];
        assert_eq!(
            angles.map(|angle| [sin(angle).to_bits(), cos(angle).to_bits()]),
            [
                [0, 0x3f800000],
                [0, 0x3f800000],
                [0, 0x3f800000],
                [0x3f800000, 0x250d3132],
                [0xbf800000, 0],
                [0x250d3132, 0xbf800000],
                [0x250d3132, 0xbf7fffd4],
                [0, 0x3f800000],
                [0, 0x3f7fffd4],
                [0xbe8e398f, 0x3f75ecc2],
                [0x3e8efaad, 0x3f75d0c2],
                [0xba48f443, 0xba48f443],
                [0, 0],
                [0, 0],
                [0xba48f443, 0xba48f443],
                [0, 0],
                [0x3f35287b, 0x3f34e165],
                [0xbf35287d, 0xbf349a32],
            ]
        );
    }

    #[test]
    fn selectors_match_engine_0_3_0() {
        // Java `SelectorSweep` oracle (`.local/terraforged/oracles/selectors/`).
        let source = |kind, seed, scale: f32, octaves, gain| {
            Module::source(Source::new(
                kind,
                SourceConfig {
                    seed,
                    octaves,
                    gain,
                    frequency: 1.0 / scale,
                    lacunarity: 2.0,
                    interpolation: Interpolation::Curve3,
                },
            ))
        };
        let control = source(SourceKind::Perlin, 501, 80.0, 3, 0.5);
        let lower = source(SourceKind::Simplex, 502, 120.0, 2, 0.5)
            .scale(Module::constant(0.5))
            .bias(Module::constant(-0.25));
        let upper = source(SourceKind::Ridge, 503, 96.0, 3, 0.975)
            .scale(Module::constant(1.5))
            .bias(Module::constant(0.1));
        let variable = source(SourceKind::Simplex, 504, 64.0, 2, 0.5);
        let points = [
            (0.0, 0.0),
            (12.25, -98.75),
            (-4096.5, 8192.25),
            (123456.75, -0.125),
            (64.0, 64.0),
            (-128.0, 32.0),
            (777.0, -333.0),
            (-1024.0, -2048.0),
        ];
        let cases = [
            (
                lower
                    .clone()
                    .base(upper.clone(), 0.3, Interpolation::Curve3),
                [0xbe800000, 0x3fcccccd],
                [
                    0x3fcccccd, 0x3fa8e9ab, 0x3f964679, 0x3fc8a719, 0x3f46bead, 0x3f773e11,
                    0x3fafdf2f, 0x3f73d5fa,
                ],
            ),
            (
                control.clone().blend(
                    lower.clone(),
                    upper.clone(),
                    0.45,
                    0.35,
                    Interpolation::Curve4,
                ),
                [0xbe800000, 0x3fcccccd],
                [
                    0x3f9a5848, 0x3f2114d2, 0x3f6a1af9, 0x3fc709c5, 0x3e058e80, 0x3f772288,
                    0x3fa8c9bc, 0x3c912b30,
                ],
            ),
            (
                control.clone().variable_blend(
                    variable,
                    lower.clone(),
                    upper.clone(),
                    0.55,
                    0.05,
                    0.2,
                    Interpolation::Curve3,
                ),
                [0xbe800000, 0x3fcccccd],
                [
                    0x3ed46297, 0xbdd54186, 0x3e5881e1, 0x3f876c7b, 0x3c197ec0, 0x3f464b90,
                    0x3f4c3d46, 0xbdf59262,
                ],
            ),
            (
                control.clone().select(
                    lower.clone(),
                    upper.clone(),
                    0.3,
                    0.7,
                    0.12,
                    Interpolation::Curve4,
                ),
                [0xbe800000, 0x3fcccccd],
                [
                    0x3fcccccd, 0x3fa8e9ab, 0x3f964679, 0x3fc868e9, 0x3f3f186f, 0x3f728147,
                    0x3fafdf2f, 0x3f626264,
                ],
            ),
            (
                control
                    .clone()
                    .select(lower, upper, 0.3, 0.7, 0.0, Interpolation::Linear),
                [0xbe800000, 0x3fcccccd],
                [
                    0, 0xbe04fbbe, 0xbda942b6, 0xbe166d16, 0x3c197ec0, 0x3cf473f0, 0x3dcc8d18,
                    0xbdf59262,
                ],
            ),
        ];
        for (module, expected_bounds, expected_values) in cases {
            assert_eq!(
                [module.min_value().to_bits(), module.max_value().to_bits()],
                expected_bounds
            );
            assert_eq!(
                bits(points.map(|(x, y)| module.value(x, y))),
                expected_values
            );
        }
    }

    #[test]
    fn selector_edges_match_engine_0_3_0() {
        // Java `Selectors` oracle (`.local/terraforged/oracles/selectors/`): constant controls on and between the
        // edges, reversed bounds, zero and negative falloffs. NaN prints as `floatToIntBits` (0x7fc00000).
        const EXPECTED: &str = "\
sel0 0x3dcccccd 0xbe800000 0x3fcccccd | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
sel0inv 0x3dcccccd 0xbe800000 0x3fcccccd | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
sel 0x3dcccccd 0xbe800000 0x3fcccccd | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
selinv 0x3dcccccd 0xbe800000 0x3fcccccd | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
selneg 0x3dcccccd 0xbe800000 0x3fcccccd | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
blend 0x3dcccccd 0xbe800000 0x3fcccccd | 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000\n\
var 0x3dcccccd 0xbe800000 0x3fcccccd | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
base 0x3dcccccd 0x3dcccccd 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
base0 0x3dcccccd 0x3dcccccd 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
baseneg 0x3dcccccd 0x3dcccccd 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
sel0 0x3e99999a 0xbe800000 0x3fcccccd | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
sel0inv 0x3e99999a 0xbe800000 0x3fcccccd | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
sel 0x3e99999a 0xbe800000 0x3fcccccd | 0x3f4ccccd 0x3f184a34 0x3f0bb24d 0x3ec924a8\n\
selinv 0x3e99999a 0xbe800000 0x3fcccccd | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
selneg 0x3e99999a 0xbe800000 0x3fcccccd | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
blend 0x3e99999a 0xbe800000 0x3fcccccd | 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000\n\
var 0x3e99999a 0xbe800000 0x3fcccccd | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
base 0x3e99999a 0x3e99999a 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
base0 0x3e99999a 0x3e99999a 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
baseneg 0x3e99999a 0x3e99999a 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
sel0 0x3eb851ec 0xbe800000 0x3fcccccd | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
sel0inv 0x3eb851ec 0xbe800000 0x3fcccccd | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
sel 0x3eb851ec 0xbe800000 0x3fcccccd | 0x3faccccd 0x3f8bec3e 0x3f7a489d 0x3f2810d1\n\
selinv 0x3eb851ec 0xbe800000 0x3fcccccd | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
selneg 0x3eb851ec 0xbe800000 0x3fcccccd | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
blend 0x3eb851ec 0xbe800000 0x3fcccccd | 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000\n\
var 0x3eb851ec 0xbe800000 0x3fcccccd | 0x3e75c292 0xbdd3fd70 0xbda942b6 0x3c197ec0\n\
base 0x3eb851ec 0x3eb851ec 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
base0 0x3eb851ec 0x3eb851ec 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
baseneg 0x3eb851ec 0x3eb851ec 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
sel0 0x3f000000 0xbe800000 0x3fcccccd | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
sel0inv 0x3f000000 0xbe800000 0x3fcccccd | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
sel 0x3f000000 0xbe800000 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
selinv 0x3f000000 0xbe800000 0x3fcccccd | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
selneg 0x3f000000 0xbe800000 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
blend 0x3f000000 0xbe800000 0x3fcccccd | 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000\n\
var 0x3f000000 0xbe800000 0x3fcccccd | 0x3f4ccccd 0x3f184a34 0x3f0bb24d 0x3ec924a8\n\
base 0x3f000000 0x3f000000 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f457eaf\n\
base0 0x3f000000 0x3f000000 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
baseneg 0x3f000000 0x3f000000 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
sel0 0x3f333333 0xbe800000 0x3fcccccd | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
sel0inv 0x3f333333 0xbe800000 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
sel 0x3f333333 0xbe800000 0x3fcccccd | 0x3f4ccccd 0x3f184a33 0x3f0bb24e 0x3ec924a8\n\
selinv 0x3f333333 0xbe800000 0x3fcccccd | 0x3f4ccccd 0x3f184a34 0x3f0bb24d 0x3ec924a8\n\
selneg 0x3f333333 0xbe800000 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
blend 0x3f333333 0xbe800000 0x3fcccccd | 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000\n\
var 0x3f333333 0xbe800000 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
base 0x3f333333 0x3f333333 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f365a6b\n\
base0 0x3f333333 0x3f333333 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
baseneg 0x3f333333 0x3f333333 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
sel0 0x3f51eb85 0xbe800000 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
sel0inv 0x3f51eb85 0xbe800000 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
sel 0x3f51eb85 0xbe800000 0x3fcccccd | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
selinv 0x3f51eb85 0xbe800000 0x3fcccccd | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
selneg 0x3f51eb85 0xbe800000 0x3fcccccd | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
blend 0x3f51eb85 0xbe800000 0x3fcccccd | 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000\n\
var 0x3f51eb85 0xbe800000 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
base 0x3f51eb85 0x3f51eb85 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f51eb85\n\
base0 0x3f51eb85 0x3f51eb85 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f51eb85\n\
baseneg 0x3f51eb85 0x3f51eb85 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
sel0 0x3f666666 0xbe800000 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
sel0inv 0x3f666666 0xbe800000 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
sel 0x3f666666 0xbe800000 0x3fcccccd | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
selinv 0x3f666666 0xbe800000 0x3fcccccd | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
selneg 0x3f666666 0xbe800000 0x3fcccccd | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
blend 0x3f666666 0xbe800000 0x3fcccccd | 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000\n\
var 0x3f666666 0xbe800000 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
base 0x3f666666 0x3f666666 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f958832 0x3f666666\n\
base0 0x3f666666 0x3f666666 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f666666\n\
baseneg 0x3f666666 0x3f666666 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
sel0 0x7fc00000 0xbe800000 0x3fcccccd | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
sel0inv 0x7fc00000 0xbe800000 0x3fcccccd | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
sel 0x7fc00000 0xbe800000 0x3fcccccd | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
selinv 0x7fc00000 0xbe800000 0x3fcccccd | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
selneg 0x7fc00000 0xbe800000 0x3fcccccd | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
blend 0x7fc00000 0xbe800000 0x3fcccccd | 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000\n\
var 0x7fc00000 0xbe800000 0x3fcccccd | 0x7fc00000 0x7fc00000 0x7fc00000 0x7fc00000\n\
base 0x7fc00000 0x7fc00000 0x7fc00000 | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
base0 0x7fc00000 0x7fc00000 0x7fc00000 | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
baseneg 0x7fc00000 0x7fc00000 0x7fc00000 | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
blendwide 0xbe800000 0x3fcccccd | 0x3f4ccccd 0x3f0760c8 0x3f0ea1c7 0x3e9978a0\n\
blendzero 0xbe800000 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3c197ec0\n\
basectl 0xbe800000 0x3f800000 | 0x3eb98c02 0x3e32940c 0x3ebc62d2 0x3da4e1f8";
        let source = |kind, seed, scale: f32, octaves, gain| {
            Module::source(Source::new(
                kind,
                SourceConfig {
                    seed,
                    octaves,
                    gain,
                    frequency: 1.0 / scale,
                    lacunarity: 2.0,
                    interpolation: Interpolation::Curve3,
                },
            ))
        };
        let hex = |value: f32| {
            let bits = if value.is_nan() {
                0x7fc0_0000
            } else {
                value.to_bits()
            };
            format!("0x{bits:08x}")
        };
        let mut lines = Vec::new();
        let mut print = |name: String, module: Module| {
            let mut line = format!(
                "{name} {} {} |",
                hex(module.min_value()),
                hex(module.max_value())
            );
            for (x, y) in [
                (0.0, 0.0),
                (12.25, -98.75),
                (-4096.5, 8192.25),
                (64.0, 64.0),
            ] {
                line += &format!(" {}", hex(module.value(x, y)));
            }
            lines.push(line);
        };
        let lower = source(SourceKind::Simplex, 502, 120.0, 2, 0.5)
            .scale(Module::constant(0.5))
            .bias(Module::constant(-0.25));
        let upper = source(SourceKind::Ridge, 503, 96.0, 3, 0.975)
            .scale(Module::constant(1.5))
            .bias(Module::constant(0.1));
        let variable = source(SourceKind::Simplex, 504, 64.0, 2, 0.5);
        let (l, u) = (|| lower.clone(), || upper.clone());
        for c in [0.1, 0.3, 0.36, 0.5, 0.7, 0.82, 0.9, f32::NAN] {
            let k = || Module::constant(c);
            let t = hex(c);
            print(
                format!("sel0 {t}"),
                k().select(l(), u(), 0.3, 0.7, 0.0, Interpolation::Linear),
            );
            print(
                format!("sel0inv {t}"),
                k().select(l(), u(), 0.7, 0.3, 0.0, Interpolation::Linear),
            );
            print(
                format!("sel {t}"),
                k().select(l(), u(), 0.3, 0.7, 0.12, Interpolation::Curve3),
            );
            print(
                format!("selinv {t}"),
                k().select(l(), u(), 0.7, 0.3, 0.12, Interpolation::Curve4),
            );
            print(
                format!("selneg {t}"),
                k().select(l(), u(), 0.3, 0.7, -0.12, Interpolation::Linear),
            );
            print(
                format!("blend {t}"),
                k().blend(l(), u(), 0.5, 0.2, Interpolation::Curve3),
            );
            let var = k().variable_blend(
                variable.clone(),
                l(),
                u(),
                0.5,
                0.05,
                0.3,
                Interpolation::Linear,
            );
            print(format!("var {t}"), var);
            print(
                format!("base {t}"),
                k().base(u(), 0.3, Interpolation::Curve3),
            );
            print(
                format!("base0 {t}"),
                k().base(u(), 0.0, Interpolation::Curve3),
            );
            print(
                format!("baseneg {t}"),
                k().base(u(), -0.2, Interpolation::Linear),
            );
        }
        let control = source(SourceKind::Perlin, 501, 80.0, 3, 0.5);
        print(
            "blendwide".into(),
            control
                .clone()
                .blend(l(), u(), 0.45, 5.0, Interpolation::Linear),
        );
        print(
            "blendzero".into(),
            control
                .clone()
                .blend(l(), u(), 0.45, 0.0, Interpolation::Linear),
        );
        print(
            "basectl".into(),
            l().base(control, 0.4, Interpolation::Curve4),
        );
        assert_eq!(lines, EXPECTED.lines().collect::<Vec<_>>());
    }

    #[test]
    fn multi_blend_matches_engine_0_3_0() {
        let sources = || {
            vec![
                Module::constant(-1.0),
                Module::constant(0.25),
                Module::constant(2.0),
            ]
        };
        let points = [
            (0.0, 0.0),
            (12.25, -98.75),
            (-4096.5, 8192.25),
            (123456.75, -0.125),
            (64.0, 64.0),
            (-128.0, 32.0),
            (777.0, -333.0),
            (-1024.0, -2048.0),
        ];
        let control = Module::source(Source::new(
            SourceKind::Perlin,
            SourceConfig {
                seed: 501,
                octaves: 3,
                gain: 0.5,
                frequency: 1.0 / 80.0,
                lacunarity: 2.0,
                interpolation: Interpolation::Curve3,
            },
        ));
        let module = control.multi_blend(0.4, Interpolation::Curve3, sources());
        assert_eq!(
            [module.min_value().to_bits(), module.max_value().to_bits()],
            [0xbf800000, 0x40000000]
        );
        assert_eq!(
            bits(points.map(|(x, y)| module.value(x, y))),
            [
                0x3e800000, 0x3e800000, 0x3e800000, 0x3fb9d3fa, 0x3e800000, 0x3ffd47bb, 0x3ece61c4,
                0x3e800000,
            ]
        );

        let selectors = [
            0.0, 0.21666665, 0.21666667, 0.25, 0.2833333, 0.28333336, 0.5, 0.55, 0.5833333,
            0.6166666, 0.6166667, 0.75, 1.0,
        ];
        assert_eq!(
            selectors.map(|selector| {
                Module::constant(selector)
                    .multi_blend(0.4, Interpolation::Curve3, sources())
                    .value(0.0, 0.0)
                    .to_bits()
            }),
            [
                0xbf800000, 0xbf800000, 0xbf800000, 0xbec00004, 0x3e800000, 0x3e800000, 0x3e800000,
                0x3e800000, 0x3f8ffff4, 0x40000000, 0x40000000, 0x40000000, 0x40000000,
            ]
        );
        assert_eq!(
            [0.49, 0.5, 0.51].map(|selector| {
                Module::constant(selector)
                    .multi_blend(0.0, Interpolation::Curve3, sources())
                    .value(0.0, 0.0)
                    .to_bits()
            }),
            [0x3e800000; 3]
        );
    }

    #[test]
    fn multi_blend_edges_match_engine_0_3_0() {
        // Java `MultiBlends` oracle (`.local/terraforged/oracles/multiblend/`): constant controls across 0..1, -0.0
        // and NaN, a blend above 1, a negative blend, a -0.0 source and noise controls. NaN prints as
        // `floatToIntBits` (0x7fc00000).
        const EXPECTED: &str = "\
n2 b0x3fc00000 0x00000000 0xbe800000 0x3fcccccd | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
n2 b0x3fc00000 0x3d4ccccd 0xbe800000 0x3fcccccd | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
n2 b0x3fc00000 0x3dcccccd 0xbe800000 0x3fcccccd | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
n2 b0x3fc00000 0x3e4ccccd 0xbe800000 0x3fcccccd | 0x3d5a7412 0xbda703ac 0xbd26f187 0x3d0f1788\n\
n2 b0x3fc00000 0x3e800000 0xbe800000 0x3fcccccd | 0x3e888889 0x3de4cb8c 0x3e01d780 0x3e0c7db8\n\
n2 b0x3fc00000 0x3e99999a 0xbe800000 0x3fcccccd | 0x3ef5c292 0x3e9c26b3 0x3e96b5b2 0x3e753591\n\
n2 b0x3fc00000 0x3eb33333 0xbe800000 0x3fcccccd | 0x3f317e4b 0x3eff1a7f 0x3eec7fa0 0x3eaef6b2\n\
n2 b0x3fc00000 0x3ecccccd 0xbe800000 0x3fcccccd | 0x3f681b4f 0x3f310728 0x3f2124c9 0x3ee3529d\n\
n2 b0x3fc00000 0x3ee66666 0xbe800000 0x3fcccccd | 0x3f8f5c29 0x3f62810e 0x3f4c09c2 0x3f0bd744\n\
n2 b0x3fc00000 0x3f000000 0xbe800000 0x3fcccccd | 0x3faaaaab 0x3f89fd7a 0x3f76eeb9 0x3f26053a\n\
n2 b0x3fc00000 0x3f0ccccd 0xbe800000 0x3fcccccd | 0x3fc5f92d 0x3fa2ba6e 0x3f90e9da 0x3f403330\n\
n2 b0x3fc00000 0x3f19999a 0xbe800000 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
n2 b0x3fc00000 0x3f266666 0xbe800000 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
n2 b0x3fc00000 0x3f333333 0xbe800000 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
n2 b0x3fc00000 0x3f400000 0xbe800000 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
n2 b0x3fc00000 0x3f4ccccd 0xbe800000 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
n2 b0x3fc00000 0x3f666666 0xbe800000 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
n2 b0x3fc00000 0x3f733333 0xbe800000 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
n2 b0x3fc00000 0x3f800000 0xbe800000 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
n2 b0x3fc00000 0x80000000 0xbe800000 0x3fcccccd | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
n2 b0x3fc00000 0x7fc00000 0xbe800000 0x3fcccccd | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
n3 b0x3ecccccd 0x00000000 0xbe800000 0x3fcccccd | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
n3 b0x3ecccccd 0x3d4ccccd 0xbe800000 0x3fcccccd | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
n3 b0x3ecccccd 0x3dcccccd 0xbe800000 0x3fcccccd | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
n3 b0x3ecccccd 0x3e4ccccd 0xbe800000 0x3fcccccd | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
n3 b0x3ecccccd 0x3e800000 0xbe800000 0x3fcccccd | 0x3f4cccca 0x3f184a30 0x3f0bb24a 0x3ec924a5\n\
n3 b0x3ecccccd 0x3e99999a 0xbe800000 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
n3 b0x3ecccccd 0x3eb33333 0xbe800000 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
n3 b0x3ecccccd 0x3ecccccd 0xbe800000 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
n3 b0x3ecccccd 0x3ee66666 0xbe800000 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
n3 b0x3ecccccd 0x3f000000 0xbe800000 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
n3 b0x3ecccccd 0x3f0ccccd 0xbe800000 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
n3 b0x3ecccccd 0x3f19999a 0xbe800000 0x3fcccccd | 0x3f2bfffe 0x3f2db6f1 0x3f1bef33 0x3f0b0dca\n\
n3 b0x3ecccccd 0x3f266666 0xbe800000 0x3fcccccd | 0x3f000000 0x3f0f52e0 0x3f012753 0x3f000000\n\
n3 b0x3ecccccd 0x3f333333 0xbe800000 0x3fcccccd | 0x3f000000 0x3f0f52e0 0x3f012753 0x3f000000\n\
n3 b0x3ecccccd 0x3f400000 0xbe800000 0x3fcccccd | 0x3f000000 0x3f0f52e0 0x3f012753 0x3f000000\n\
n3 b0x3ecccccd 0x3f4ccccd 0xbe800000 0x3fcccccd | 0x3f000000 0x3f0f52e0 0x3f012753 0x3f000000\n\
n3 b0x3ecccccd 0x3f666666 0xbe800000 0x3fcccccd | 0x3f000000 0x3f0f52e0 0x3f012753 0x3f000000\n\
n3 b0x3ecccccd 0x3f733333 0xbe800000 0x3fcccccd | 0x3f000000 0x3f0f52e0 0x3f012753 0x3f000000\n\
n3 b0x3ecccccd 0x3f800000 0xbe800000 0x3fcccccd | 0x3f000000 0x3f0f52e0 0x3f012753 0x3f000000\n\
n3 b0x3ecccccd 0x80000000 0xbe800000 0x3fcccccd | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
n3 b0x3ecccccd 0x7fc00000 0xbe800000 0x3fcccccd | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
n5 b0xbe99999a 0x00000000 0xbe800000 0x40600000 | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
n5 b0xbe99999a 0x3d4ccccd 0xbe800000 0x40600000 | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
n5 b0xbe99999a 0x3dcccccd 0xbe800000 0x40600000 | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
n5 b0xbe99999a 0x3e4ccccd 0xbe800000 0x40600000 | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
n5 b0xbe99999a 0x3e800000 0xbe800000 0x40600000 | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
n5 b0xbe99999a 0x3e99999a 0xbe800000 0x40600000 | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
n5 b0xbe99999a 0x3eb33333 0xbe800000 0x40600000 | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
n5 b0xbe99999a 0x3ecccccd 0xbe800000 0x40600000 | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
n5 b0xbe99999a 0x3ee66666 0xbe800000 0x40600000 | 0x3f000000 0x3f0f52e0 0x3f012753 0x3f000000\n\
n5 b0xbe99999a 0x3f000000 0xbe800000 0x40600000 | 0x3f000000 0x3f0f52e0 0x3f012753 0x3f000000\n\
n5 b0xbe99999a 0x3f0ccccd 0xbe800000 0x40600000 | 0x3f000000 0x3f0f52e0 0x3f012753 0x3f000000\n\
n5 b0xbe99999a 0x3f19999a 0xbe800000 0x40600000 | 0x3f000000 0x3f0f52e0 0x3f012753 0x3f000000\n\
n5 b0xbe99999a 0x3f266666 0xbe800000 0x40600000 | 0x80000000 0x80000000 0x80000000 0x80000000\n\
n5 b0xbe99999a 0x3f333333 0xbe800000 0x40600000 | 0x80000000 0x80000000 0x80000000 0x80000000\n\
n5 b0xbe99999a 0x3f400000 0xbe800000 0x40600000 | 0x80000000 0x80000000 0x80000000 0x80000000\n\
n5 b0xbe99999a 0x3f4ccccd 0xbe800000 0x40600000 | 0x00000000 0x00000000 0x00000000 0x00000000\n\
n5 b0xbe99999a 0x3f666666 0xbe800000 0x40600000 | 0x40600000 0x40600000 0x40600000 0x40600000\n\
n5 b0xbe99999a 0x3f733333 0xbe800000 0x40600000 | 0x40600000 0x40600000 0x40600000 0x40600000\n\
n5 b0xbe99999a 0x3f800000 0xbe800000 0x40600000 | 0x40600000 0x40600000 0x40600000 0x40600000\n\
n5 b0xbe99999a 0x80000000 0xbe800000 0x40600000 | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
n5 b0xbe99999a 0x7fc00000 0xbe800000 0x40600000 | 0x00000000 0xbe04fbbe 0xbda942b6 0x3c197ec0\n\
ctl3 0xbe800000 0x3fcccccd | 0x3fcccccd 0x3fa8e9ab 0x3f964679 0x3f46bead\n\
ctl5 0xbe800000 0x40600000 | 0x3f000000 0x3f0f52e0 0x3f012753 0x3f0a143e";
        let source = |kind, seed, scale: f32, octaves, gain| {
            Module::source(Source::new(
                kind,
                SourceConfig {
                    seed,
                    octaves,
                    gain,
                    frequency: 1.0 / scale,
                    lacunarity: 2.0,
                    interpolation: Interpolation::Curve3,
                },
            ))
        };
        let hex = |value: f32| {
            let bits = if value.is_nan() {
                0x7fc0_0000
            } else {
                value.to_bits()
            };
            format!("0x{bits:08x}")
        };
        let mut lines = Vec::new();
        let mut print = |name: String, module: Module| {
            let mut line = format!(
                "{name} {} {} |",
                hex(module.min_value()),
                hex(module.max_value())
            );
            for (x, y) in [
                (0.0, 0.0),
                (12.25, -98.75),
                (-4096.5, 8192.25),
                (64.0, 64.0),
            ] {
                line += &format!(" {}", hex(module.value(x, y)));
            }
            lines.push(line);
        };
        let sources = [
            source(SourceKind::Simplex, 502, 120.0, 2, 0.5)
                .scale(Module::constant(0.5))
                .bias(Module::constant(-0.25)),
            source(SourceKind::Ridge, 503, 96.0, 3, 0.975)
                .scale(Module::constant(1.5))
                .bias(Module::constant(0.1)),
            source(SourceKind::Perlin, 504, 64.0, 2, 0.5),
            Module::constant(-0.0),
            Module::constant(3.5),
        ];
        let controls = [
            0.0,
            0.05,
            0.1,
            0.2,
            0.25,
            0.3,
            0.35,
            0.4,
            0.45,
            0.5,
            0.55,
            0.6,
            0.65,
            0.7,
            0.75,
            0.8,
            0.9,
            0.95,
            1.0,
            -0.0,
            f32::NAN,
        ];
        for (count, blend, interpolation) in [
            (2, 1.5, Interpolation::Linear),
            (3, 0.4, Interpolation::Curve3),
            (5, -0.3, Interpolation::Curve3),
        ] {
            for control in controls {
                print(
                    format!("n{count} b{} {}", hex(blend), hex(control)),
                    Module::constant(control).multi_blend(
                        blend,
                        interpolation,
                        sources[..count].to_vec(),
                    ),
                );
            }
        }
        let control = source(SourceKind::Perlin, 501, 80.0, 3, 0.5);
        print(
            "ctl3".into(),
            control
                .clone()
                .multi_blend(0.4, Interpolation::Curve3, sources[..3].to_vec()),
        );
        print(
            "ctl5".into(),
            control.multi_blend(0.8, Interpolation::Linear, sources.to_vec()),
        );
        assert_eq!(lines, EXPECTED.lines().collect::<Vec<_>>());

        // Engine's `Combiner` bounds without sources.
        let empty = Module::constant(0.5).multi_blend(0.4, Interpolation::Linear, Vec::new());
        assert_eq!([empty.min_value(), empty.max_value()], [0.0, 0.0]);
    }

    #[test]
    #[should_panic(expected = "Engine throws")]
    fn multi_blend_panics_where_engine_throws() {
        // Engine: `nodes[index - 1]` with index 0 throws `ArrayIndexOutOfBoundsException`.
        Module::constant(-0.05)
            .multi_blend(0.4, Interpolation::Linear, vec![Module::constant(1.0); 3])
            .value(0.0, 0.0);
    }
}
