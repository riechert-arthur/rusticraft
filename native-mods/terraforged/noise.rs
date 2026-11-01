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

#[derive(Clone, Copy)]
pub enum Interpolation {
    Linear,
    Curve3,
    Curve4,
}

impl Interpolation {
    fn apply(self, value: f32) -> f32 {
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

pub fn floor(value: f32) -> i32 {
    if value >= 0.0 {
        value as i32
    } else {
        value as i32 - 1
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

pub fn perlin(x: f32, y: f32, seed: i32, interpolation: Interpolation) -> f32 {
    let x0 = floor(x);
    let y0 = floor(y);
    let x1 = x0 + 1;
    let y1 = y0 + 1;
    let xs = interpolation.apply(x - x0 as f32);
    let ys = interpolation.apply(y - y0 as f32);
    let xd0 = x - x0 as f32;
    let yd0 = y - y0 as f32;
    let xd1 = xd0 - 1.0;
    let yd1 = yd0 - 1.0;
    let lower = lerp(
        grad(seed, x0, y0, xd0, yd0, false),
        grad(seed, x1, y0, xd1, yd0, false),
        xs,
    );
    let upper = lerp(
        grad(seed, x0, y1, xd0, yd1, false),
        grad(seed, x1, y1, xd1, yd1, false),
        xs,
    );
    lerp(lower, upper, ys)
}

pub fn perlin2(x: f32, y: f32, seed: i32, interpolation: Interpolation) -> f32 {
    let x0 = floor(x);
    let y0 = floor(y);
    let x1 = x0 + 1;
    let y1 = y0 + 1;
    let xs = interpolation.apply(x - x0 as f32);
    let ys = interpolation.apply(y - y0 as f32);
    let xd0 = x - x0 as f32;
    let yd0 = y - y0 as f32;
    let xd1 = xd0 - 1.0;
    let yd1 = yd0 - 1.0;
    let lower = lerp(
        grad(seed, x0, y0, xd0, yd0, true),
        grad(seed, x1, y0, xd1, yd0, true),
        xs,
    );
    let upper = lerp(
        grad(seed, x0, y1, xd0, yd1, true),
        grad(seed, x1, y1, xd1, yd1, true),
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
            + corner(seed, i + i1, j + j1, x1, y1)
            + corner(seed, i + 1, j + 1, x2, y2))
}

fn cubic_lerp(a: f32, b: f32, c: f32, d: f32, t: f32) -> f32 {
    let p = d - c - (a - b);
    t * t * t * p + t * t * (a - b - p) + t * (c - a) + b
}

pub fn cubic(x: f32, y: f32, seed: i32) -> f32 {
    let x1 = floor(x);
    let y1 = floor(y);
    let rows = [-1, 0, 1, 2].map(|dy| {
        let values = [-1, 0, 1, 2].map(|dx| value_coord_2d(seed, x1 + dx, y1 + dy));
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
        Self {
            config,
            kind,
            min,
            max,
            range: max - min,
        }
    }

    pub fn value(self, x: f32, y: f32) -> f32 {
        self.evaluate(x, y, self.config.seed, false)
    }

    pub fn value_seeded(self, x: f32, y: f32, seed: i32) -> f32 {
        self.evaluate(x, y, seed, true)
    }

    pub fn min_value(self) -> f32 {
        0.0
    }

    pub fn max_value(self) -> f32 {
        1.0
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
                value += signal * spectral_frequency.powf(-1.0);
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
        value += noise * spectral_frequency.powf(-1.0);
        spectral_frequency *= config.lacunarity;
        amplitude *= config.gain;
    }
    value
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

#[derive(Clone)]
pub struct Module(Box<ModuleKind>);

#[derive(Clone)]
struct BlendNode {
    source: Module,
    min: f32,
    max: f32,
}

#[derive(Clone)]
struct TerraceStep {
    value: f32,
    lower_bound: f32,
    upper_bound: f32,
}

#[derive(Clone)]
enum ModuleKind {
    Constant(f32),
    Source(Source),
    Abs(Module),
    Alpha(Module, Module),
    Bias(Module, Module),
    Boost(Module, usize),
    Clamp(Module, Module, Module),
    Invert(Module),
    Map(Module, Module, Module),
    Power(Module, Module),
    Scale(Module, Module, f32, f32),
    Threshold(Module, Module),
    Warp(Module, Domain),
    Base {
        base: Module,
        source: Module,
        min: f32,
        max: f32,
        falloff: f32,
        max_value: f32,
        interpolation: Interpolation,
    },
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
    MultiBlend {
        control: Module,
        nodes: Vec<BlendNode>,
        max_index: usize,
        blend_range: f32,
        interpolation: Interpolation,
    },
    Terrace {
        source: Module,
        ramp: Module,
        cliff: Module,
        ramp_height: Module,
        steps: Vec<TerraceStep>,
        max_index: usize,
    },
    LegacyTerrace {
        source: Module,
        lower_curve: Module,
        upper_curve: Module,
        steps: Vec<TerraceStep>,
        max_index: usize,
    },
    Select {
        control: Module,
        lower: Module,
        upper: Module,
        lower_bound: f32,
        upper_bound: f32,
        falloff: f32,
        interpolation: Interpolation,
    },
    Add(Vec<Module>),
    Multiply(Vec<Module>),
    Max(Vec<Module>),
}

impl Module {
    pub fn constant(value: f32) -> Self {
        Self(Box::new(ModuleKind::Constant(value)))
    }

    pub fn source(source: Source) -> Self {
        Self(Box::new(ModuleKind::Source(source)))
    }

    pub fn value(&self, x: f32, y: f32) -> f32 {
        match self.0.as_ref() {
            ModuleKind::Constant(value) => *value,
            ModuleKind::Source(source) => source.value(x, y),
            ModuleKind::Abs(source) => source.value(x, y).abs(),
            ModuleKind::Alpha(source, alpha) => {
                let alpha = alpha.value(x, y);
                source.value(x, y) * alpha + (1.0 - alpha)
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
                clamp(source.value(x, y), min.value(x, y), max.value(x, y))
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
            ModuleKind::Map(source, min, max) => {
                let alpha = (source.value(x, y) - source.min_value())
                    / (source.max_value() - source.min_value());
                let min = min.value(x, y);
                let max = max.value(x, y);
                min + alpha * (max - min)
            }
            ModuleKind::Power(source, power) => java_pow(source.value(x, y), power.value(x, y)),
            ModuleKind::Scale(source, scale, ..) => source.value(x, y) * scale.value(x, y),
            ModuleKind::Threshold(source, threshold) => {
                if source.value(x, y) < threshold.value(x, y) {
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
                ..
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
                let index = round(selector * *max_index as f32) as usize;
                let mut min = &nodes[index];
                let mut max = min;
                if *blend_range == 0.0 {
                    return min.source.value(x, y);
                }
                if selector > min.max {
                    max = &nodes[index + 1];
                } else if selector < min.min {
                    min = &nodes[index - 1];
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
            ModuleKind::Terrace {
                source,
                ramp,
                cliff,
                ramp_height,
                steps,
                max_index,
            } => {
                let noise_value = clamp(source.value(x, y), 0.0, 0.999999);
                let index = floor(noise_value * steps.len() as f32) as usize;
                let step = &steps[index];
                if index == *max_index || noise_value < step.lower_bound {
                    return step.value;
                }
                if noise_value > step.upper_bound {
                    return steps[index + 1].value;
                }
                let ramp = 1.0 - ramp.value(x, y) * 0.5;
                let cliff = 1.0 - cliff.value(x, y) * 0.5;
                let alpha =
                    (noise_value - step.lower_bound) / (step.upper_bound - step.lower_bound);
                let mut value = step.value;
                if alpha > ramp {
                    let next = &steps[index + 1];
                    let ramp_size = 1.0 - ramp;
                    let ramp_alpha = (alpha - ramp) / ramp_size;
                    value += (next.value - value) * ramp_alpha * ramp_height.value(x, y);
                }
                if alpha > cliff {
                    let next = &steps[index + 1];
                    let cliff_alpha = (alpha - cliff) / (1.0 - cliff);
                    value = lerp(value, next.value, cliff_alpha);
                }
                value
            }
            ModuleKind::LegacyTerrace {
                source,
                lower_curve,
                upper_curve,
                steps,
                max_index,
            } => {
                let noise_value = clamp(source.value(x, y), 0.0, 1.0);
                let index = round(noise_value * *max_index as f32) as usize;
                let step = &steps[index];
                if noise_value < step.lower_bound && index > 0 {
                    let lower = &steps[index - 1];
                    let alpha =
                        (noise_value - lower.upper_bound) / (step.lower_bound - lower.upper_bound);
                    let alpha = 1.0 - Interpolation::Curve3.apply(alpha);
                    let range = step.value - lower.value;
                    return step.value - alpha * range * upper_curve.value(x, y);
                }
                if noise_value > step.upper_bound && index < *max_index {
                    let upper = &steps[index + 1];
                    let alpha =
                        (noise_value - step.upper_bound) / (upper.lower_bound - step.upper_bound);
                    let alpha = Interpolation::Curve3.apply(alpha);
                    let range = upper.value - step.value;
                    return step.value + alpha * range * lower_curve.value(x, y);
                }
                step.value
            }
            ModuleKind::Select {
                control,
                lower,
                upper,
                lower_bound,
                upper_bound,
                falloff,
                interpolation,
            } => select_value(
                control.value(x, y),
                lower,
                upper,
                x,
                y,
                *lower_bound,
                *upper_bound,
                *falloff,
                *interpolation,
            ),
            ModuleKind::Add(sources) => combine(sources, x, y, |a, b| a + b),
            ModuleKind::Multiply(sources) => combine(sources, x, y, |a, b| a * b),
            ModuleKind::Max(sources) => combine(sources, x, y, java_max),
        }
    }

    pub fn min_value(&self) -> f32 {
        match self.0.as_ref() {
            ModuleKind::Constant(value) => *value,
            ModuleKind::Source(source) => source.min_value(),
            ModuleKind::Bias(source, bias) => source.min_value() + bias.min_value(),
            ModuleKind::Boost(..) => 0.0,
            ModuleKind::Clamp(_, min, _) | ModuleKind::Map(_, min, _) => min.min_value(),
            ModuleKind::Scale(_, _, min, _) => *min,
            ModuleKind::Base { base, .. } => base.min_value(),
            ModuleKind::Blend { lower, upper, .. }
            | ModuleKind::VariableBlend { lower, upper, .. }
            | ModuleKind::Select { lower, upper, .. } => {
                java_min(lower.min_value(), upper.min_value())
            }
            ModuleKind::MultiBlend { nodes, .. } => nodes[1..]
                .iter()
                .fold(nodes[0].source.min_value(), |value, node| {
                    java_min(value, node.source.min_value())
                }),
            ModuleKind::Terrace { source, .. } | ModuleKind::LegacyTerrace { source, .. } => {
                source.min_value()
            }
            ModuleKind::Add(sources) => combine_bounds(sources, true, |a, b| a + b),
            ModuleKind::Multiply(sources) => combine_bounds(sources, true, |a, b| a * b),
            ModuleKind::Max(sources) => {
                let Some((first, rest)) = sources.split_first() else {
                    return 0.0;
                };
                rest.iter().fold(first.min_value(), |value, source| {
                    java_max(value, source.max_value())
                })
            }
            ModuleKind::Abs(source)
            | ModuleKind::Alpha(source, _)
            | ModuleKind::Invert(source)
            | ModuleKind::Power(source, _)
            | ModuleKind::Threshold(source, _)
            | ModuleKind::Warp(source, _) => source.min_value(),
        }
    }

    pub fn max_value(&self) -> f32 {
        match self.0.as_ref() {
            ModuleKind::Constant(value) => *value,
            ModuleKind::Source(source) => source.max_value(),
            ModuleKind::Bias(source, bias) => source.max_value() + bias.max_value(),
            ModuleKind::Boost(..) => 1.0,
            ModuleKind::Clamp(_, _, max) | ModuleKind::Map(_, _, max) => max.max_value(),
            ModuleKind::Scale(_, _, _, max) => *max,
            ModuleKind::Base { max_value, .. } => *max_value,
            ModuleKind::Blend { lower, upper, .. }
            | ModuleKind::VariableBlend { lower, upper, .. }
            | ModuleKind::Select { lower, upper, .. } => {
                java_max(lower.max_value(), upper.max_value())
            }
            ModuleKind::MultiBlend { nodes, .. } => nodes[1..]
                .iter()
                .fold(nodes[0].source.max_value(), |value, node| {
                    java_max(value, node.source.max_value())
                }),
            ModuleKind::Terrace { source, .. } | ModuleKind::LegacyTerrace { source, .. } => {
                source.max_value()
            }
            ModuleKind::Add(sources) => combine_bounds(sources, false, |a, b| a + b),
            ModuleKind::Multiply(sources) => combine_bounds(sources, false, |a, b| a * b),
            ModuleKind::Max(sources) => combine_bounds(sources, false, java_max),
            ModuleKind::Abs(source)
            | ModuleKind::Alpha(source, _)
            | ModuleKind::Invert(source)
            | ModuleKind::Power(source, _)
            | ModuleKind::Threshold(source, _)
            | ModuleKind::Warp(source, _) => source.max_value(),
        }
    }

    pub fn abs(self) -> Self {
        if matches!(self.0.as_ref(), ModuleKind::Abs(_)) {
            self
        } else {
            Self(Box::new(ModuleKind::Abs(self)))
        }
    }

    pub fn alpha(self, alpha: Self) -> Self {
        if alpha.min_value() < 0.0 || alpha.max_value() > 1.0 {
            self
        } else {
            Self(Box::new(ModuleKind::Alpha(self, alpha)))
        }
    }

    pub fn bias(self, bias: Self) -> Self {
        if bias.min_value() == 0.0 && bias.max_value() == 0.0 {
            self
        } else {
            Self(Box::new(ModuleKind::Bias(self, bias)))
        }
    }

    pub fn boost(self, iterations: usize) -> Self {
        if iterations == 0 {
            self
        } else {
            let mapped = self.map(Self::constant(0.0), Self::constant(1.0));
            Self(Box::new(ModuleKind::Boost(mapped, iterations)))
        }
    }

    pub fn clamp(self, min: Self, max: Self) -> Self {
        if min.min_value() == min.max_value()
            && min.min_value() == self.min_value()
            && max.min_value() == max.max_value()
            && max.max_value() == self.max_value()
        {
            self
        } else {
            Self(Box::new(ModuleKind::Clamp(self, min, max)))
        }
    }

    pub fn invert(self) -> Self {
        if matches!(self.0.as_ref(), ModuleKind::Invert(_)) {
            self
        } else {
            Self(Box::new(ModuleKind::Invert(self)))
        }
    }

    pub fn map(self, min: Self, max: Self) -> Self {
        Self(Box::new(ModuleKind::Map(self, min, max)))
    }

    pub fn power(self, power: Self) -> Self {
        Self(Box::new(ModuleKind::Power(self, power)))
    }

    pub fn scale(self, scale: Self) -> Self {
        if scale.min_value() == 1.0 && scale.max_value() == 1.0 {
            return self;
        }
        let min = self.min_value() * scale.min_value();
        let max = self.max_value() * scale.max_value();
        Self(Box::new(ModuleKind::Scale(self, scale, min, max)))
    }

    pub fn threshold(self, threshold: Self) -> Self {
        Self(Box::new(ModuleKind::Threshold(self, threshold)))
    }

    pub fn warp(self, domain: Domain) -> Self {
        Self(Box::new(ModuleKind::Warp(self, domain)))
    }

    pub fn base(self, source: Self, falloff: f32, interpolation: Interpolation) -> Self {
        let min = self.max_value();
        let max = min + falloff;
        let max_value = java_max(self.max_value(), source.max_value());
        Self(Box::new(ModuleKind::Base {
            base: self,
            source,
            min,
            max,
            falloff,
            max_value,
            interpolation,
        }))
    }

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
        Self(Box::new(ModuleKind::Blend {
            control: self,
            lower,
            upper,
            blend_lower,
            blend_upper,
            blend_range: blend_upper - blend_lower,
            interpolation,
        }))
    }

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
        Self(Box::new(ModuleKind::VariableBlend {
            control: self,
            variable,
            lower,
            upper,
            midpoint,
            min_blend,
            max_blend,
            interpolation,
        }))
    }

    pub fn multi_blend(self, blend: f32, interpolation: Interpolation, sources: Vec<Self>) -> Self {
        let spacing = 1.0 / sources.len() as f32;
        let radius = spacing / 2.0;
        let blend_range = radius * blend;
        let cell_radius = (radius - blend_range) / 2.0;
        let max_index = sources.len() - 1;
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
                let max = if index == max_index {
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
        Self(Box::new(ModuleKind::MultiBlend {
            control: self,
            nodes,
            max_index,
            blend_range,
            interpolation,
        }))
    }

    pub fn terrace(
        self,
        ramp: Self,
        cliff: Self,
        ramp_height: Self,
        step_count: usize,
        blend_range: f32,
    ) -> Self {
        let max_index = step_count - 1;
        let range = self.max_value() - self.min_value();
        let spacing = range / max_index as f32;
        let steps = (0..step_count)
            .map(|index| {
                let value = index as f32 * spacing;
                let blend = spacing * blend_range;
                let bound = (spacing - blend) / 2.0;
                TerraceStep {
                    value,
                    lower_bound: value - bound,
                    upper_bound: value + bound,
                }
            })
            .collect();
        Self(Box::new(ModuleKind::Terrace {
            source: self,
            ramp,
            cliff,
            ramp_height,
            steps,
            max_index,
        }))
    }

    pub fn legacy_terrace(
        self,
        lower_curve: Self,
        upper_curve: Self,
        step_count: usize,
        blend_range: f32,
    ) -> Self {
        let max_index = step_count - 1;
        let spacing = (self.max_value() - self.min_value()) / max_index as f32;
        let steps = (0..step_count)
            .map(|index| {
                let value = index as f32 * spacing;
                let blend = spacing * blend_range;
                let bound = (spacing - blend) / 2.0;
                TerraceStep {
                    value,
                    lower_bound: value - bound,
                    upper_bound: value + bound,
                }
            })
            .collect();
        Self(Box::new(ModuleKind::LegacyTerrace {
            source: self,
            lower_curve,
            upper_curve,
            steps,
            max_index,
        }))
    }

    pub fn select(
        self,
        lower: Self,
        upper: Self,
        lower_bound: f32,
        upper_bound: f32,
        falloff: f32,
        interpolation: Interpolation,
    ) -> Self {
        Self(Box::new(ModuleKind::Select {
            control: self,
            lower,
            upper,
            lower_bound,
            upper_bound,
            falloff,
            interpolation,
        }))
    }

    pub fn add(sources: Vec<Self>) -> Self {
        Self(Box::new(ModuleKind::Add(sources)))
    }

    pub fn multiply(sources: Vec<Self>) -> Self {
        Self(Box::new(ModuleKind::Multiply(sources)))
    }

    pub fn max(sources: Vec<Self>) -> Self {
        Self(Box::new(ModuleKind::Max(sources)))
    }
}

fn select_value(
    value: f32,
    lower: &Module,
    upper: &Module,
    x: f32,
    y: f32,
    lower_bound: f32,
    upper_bound: f32,
    falloff: f32,
    interpolation: Interpolation,
) -> f32 {
    let lower_min = lower_bound - falloff;
    let lower_max = lower_bound + falloff;
    let upper_min = upper_bound - falloff;
    let upper_max = upper_bound + falloff;
    if falloff == 0.0 {
        return if value > upper_min {
            upper.value(x, y)
        } else {
            lower.value(x, y)
        };
    }
    if value < lower_min {
        lower.value(x, y)
    } else if value < lower_max {
        let alpha = (value - lower_min) / (lower_max - lower_min);
        lerp(
            lower.value(x, y),
            upper.value(x, y),
            interpolation.apply(alpha),
        )
    } else if value < upper_min {
        upper.value(x, y)
    } else if value < upper_max {
        let alpha = (value - upper_min) / (upper_max - upper_min);
        lerp(
            upper.value(x, y),
            lower.value(x, y),
            interpolation.apply(alpha),
        )
    } else {
        lower.value(x, y)
    }
}

#[derive(Clone)]
pub struct Domain(Box<DomainKind>);

#[derive(Clone)]
enum DomainKind {
    Direct,
    DomainWarp {
        x: Module,
        y: Module,
        distance: Module,
    },
    Direction {
        direction: Module,
        strength: Module,
    },
    Add(Domain, Domain),
    Compound(Domain, Domain),
    Cumulative(Domain, Domain),
}

impl Domain {
    pub fn direct() -> Self {
        Self(Box::new(DomainKind::Direct))
    }

    pub fn domain_warp(x: Module, y: Module, distance: Module) -> Self {
        let map_axis = |axis: Module| {
            if axis.min_value() == -0.5 && axis.max_value() == 0.5 {
                axis
            } else {
                axis.map(Module::constant(-0.5), Module::constant(0.5))
            }
        };
        Self(Box::new(DomainKind::DomainWarp {
            x: map_axis(x),
            y: map_axis(y),
            distance,
        }))
    }

    pub fn direction(direction: Module, strength: Module) -> Self {
        Self(Box::new(DomainKind::Direction {
            direction,
            strength,
        }))
    }

    #[allow(clippy::should_implement_trait)]
    pub fn add(self, next: Self) -> Self {
        Self(Box::new(DomainKind::Add(self, next)))
    }

    pub fn compound(self, next: Self) -> Self {
        Self(Box::new(DomainKind::Compound(self, next)))
    }

    pub fn then(self, next: Self) -> Self {
        Self(Box::new(DomainKind::Cumulative(self, next)))
    }

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
                let angle = direction.value(x, y) * java_pi2();
                sin_lookup(angle) * strength.value(x, y)
            }
            DomainKind::Add(first, second) => first.offset_x(x, y) + second.offset_x(x, y),
            DomainKind::Compound(first, second) => second.offset_x(first.x(x, y), first.y(x, y)),
            DomainKind::Cumulative(first, second) => second.x(first.x(x, y), first.y(x, y)),
        }
    }

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
                const COS_OFFSET: f32 = f32::from_bits(0x3fc9_0ff9);
                let angle = direction.value(x, y) * java_pi2();
                sin_lookup(angle + COS_OFFSET) * strength.value(x, y)
            }
            DomainKind::Add(first, second) => first.offset_y(x, y) + second.offset_y(x, y),
            DomainKind::Compound(first, second) => second.offset_y(first.x(x, y), first.y(x, y)),
            DomainKind::Cumulative(first, second) => second.y(first.x(x, y), first.y(x, y)),
        }
    }

    pub fn x(&self, x: f32, y: f32) -> f32 {
        x + self.offset_x(x, y)
    }

    pub fn y(&self, x: f32, y: f32) -> f32 {
        y + self.offset_y(x, y)
    }
}

fn combine(sources: &[Module], x: f32, y: f32, operation: impl Fn(f32, f32) -> f32) -> f32 {
    let Some((first, rest)) = sources.split_first() else {
        return 0.0;
    };
    rest.iter().fold(first.value(x, y), |value, source| {
        operation(value, source.value(x, y))
    })
}

fn combine_bounds(sources: &[Module], minimum: bool, operation: impl Fn(f32, f32) -> f32) -> f32 {
    let Some((first, rest)) = sources.split_first() else {
        return 0.0;
    };
    let initial = if minimum {
        first.min_value()
    } else {
        first.max_value()
    };
    rest.iter().fold(initial, |value, source| {
        operation(
            value,
            if minimum {
                source.min_value()
            } else {
                source.max_value()
            },
        )
    })
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

fn java_pi2() -> f32 {
    (std::f64::consts::PI * 2.0) as f32
}

fn sin_lookup(radians: f32) -> f32 {
    const SIN_COUNT: f32 = 4096.0;
    const SIN_MASK: i32 = 4095;
    let radians_full = java_pi2();
    let index = ((radians * (SIN_COUNT / radians_full)) as i32 & SIN_MASK) as usize;
    if index & 1023 == 0 {
        let degrees = (index / 1024 * 90) as f64;
        (degrees.to_radians().sin()) as f32
    } else {
        let angle = (index as f32 + 0.5) / SIN_COUNT * radians_full;
        f64::from(angle).sin() as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bits(values: impl IntoIterator<Item = f32>) -> Vec<u32> {
        values.into_iter().map(f32::to_bits).collect()
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
            bits(points.map(|(x, y)| warped.value(x, y))),
            [0x3f4fea9c, 0x3f2d9330, 0x3f4b5329, 0x3f53bd2e]
        );
    }

    #[test]
    fn selectors_match_engine_0_3_0() {
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
    fn terrace_matches_engine_0_3_0() {
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
        let module = source(SourceKind::Perlin, 601, 96.0, 4, 0.5).terrace(
            source(SourceKind::Simplex, 602, 72.0, 3, 0.5),
            source(SourceKind::Ridge, 603, 128.0, 3, 0.975),
            source(SourceKind::Perlin, 604, 80.0, 2, 0.5),
            7,
            0.35,
        );
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
        assert_eq!(
            [module.min_value().to_bits(), module.max_value().to_bits()],
            [0x00000000, 0x3f800000]
        );
        assert_eq!(
            bits(points.map(|(x, y)| module.value(x, y))),
            [
                0x3f000000, 0x3f000000, 0x3f000000, 0x3f000000, 0x3f000000, 0x3f000000, 0x3f017a74,
                0x3e2aaaab,
            ]
        );

        let bounded =
            |value| Module::constant(value).clamp(Module::constant(0.0), Module::constant(1.0));
        let values = [
            -1.0, 0.0, 0.05416666, 0.05416667, 0.1, 0.11249999, 0.1125, 0.15, 0.16666667, 0.2,
            0.22083333, 0.22083335, 0.999999, 1.0, 2.0,
        ];
        assert_eq!(
            values.map(|value| {
                bounded(value)
                    .terrace(
                        Module::constant(1.0),
                        Module::constant(1.0),
                        Module::constant(0.5),
                        7,
                        0.35,
                    )
                    .value(0.0, 0.0)
                    .to_bits()
            }),
            [
                0x00000000, 0x00000000, 0x3e2aaaaa, 0x3e2aaaab, 0x3e2aaaab, 0x3e2aaaab, 0x3e2aaaab,
                0x3e2aaaab, 0x3e2aaaad, 0x3e93f1de, 0x3eaaaaab, 0x3eaaaaab, 0x3f800000, 0x3f800000,
                0x3f800000,
            ]
        );
    }

    #[test]
    fn legacy_terrace_matches_engine_0_3_0() {
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
        let module = source(SourceKind::Perlin, 701, 96.0, 4, 0.5).legacy_terrace(
            source(SourceKind::Simplex, 702, 72.0, 3, 0.5),
            source(SourceKind::Ridge, 703, 128.0, 3, 0.975),
            7,
            0.35,
        );
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
        assert_eq!(
            bits(points.map(|(x, y)| module.value(x, y))),
            [
                0x3f000000, 0x3ebe6cf3, 0x3f296077, 0x3f000000, 0x3e91b23e, 0x3f03e9a3, 0x3f2aaaab,
                0x3f000000,
            ]
        );
        let bounded =
            |value| Module::constant(value).clamp(Module::constant(0.0), Module::constant(1.0));
        let values = [
            -1.0, 0.0, 0.05416666, 0.05416667, 0.1, 0.1125, 0.15, 0.16666667, 0.2, 0.22083333,
            0.22083335, 0.25, 0.999999, 1.0, 2.0,
        ];
        assert_eq!(
            values.map(|value| bounded(value)
                .legacy_terrace(Module::constant(0.25), Module::constant(0.75), 7, 0.35)
                .value(0.0, 0.0)
                .to_bits()),
            [
                0, 0, 0, 0x2612f052, 0x3e1b8d8f, 0x3e2aaaab, 0x3e2aaaab, 0x3e2aaaab, 0x3e2aaaab,
                0x3e2aaaab, 0x3e2aaaab, 0x3e8aaaab, 0x3f800000, 0x3f800000, 0x3f800000,
            ]
        );
    }
}
