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
}
