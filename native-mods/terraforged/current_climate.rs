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
    terraforged::{Source, SourceConfig, SourceKind, floor, hash_2d, noise_map, sin_lookup},
    terraforged_biome_sampler::ClimateSample,
    terraforged_climate::{ClimateRange, ClimateSettings},
    terraforged_continent_cell::{CellShape, random},
    terraforged_engine::{BiomeType, Seed},
};

const MOISTURE_SIZE: f32 = 2.5;
const JITTER: f32 = 0.8;

pub struct CurrentClimateNoise {
    frequency: f32,
    warp: CurrentWarp,
    moisture: CurrentMoisture,
    temperature: CurrentTemperature,
}

struct CurrentMoisture {
    source: Source,
    power: i32,
    range: ClimateRange,
    warps: [CurrentWarp; 2],
}

struct CurrentTemperature {
    frequency: f32,
    power: i32,
    range: ClimateRange,
    warps: [CurrentWarp; 2],
}

struct CurrentWarp {
    x: CurrentAxis,
    y: CurrentAxis,
    strength: f32,
}

struct CurrentAxis {
    source: Source,
    bias: f32,
    min: f32,
    max: f32,
}

impl ClimateSettings {
    /// The climate half of `NoiseGenerator.createContinentNoise`: Engine's defaults with
    /// TerraForged's biome size, falloffs and biases.
    pub fn terraforged() -> Self {
        let mut settings = Self::default();
        settings.biome_shape.biome_size = 220;
        settings.temperature.falloff = 2;
        settings.temperature.bias = 0.1;
        settings.moisture.falloff = 1;
        settings.moisture.bias = -0.05;
        settings
    }
}

impl CurrentClimateNoise {
    pub fn new(seed: &mut Seed, settings: ClimateSettings) -> Self {
        let biome_size = settings.biome_shape.biome_size;
        let biome_frequency = 1.0 / biome_size as f32;
        let moisture_scale = round(
            settings.moisture.scale as f32 * MOISTURE_SIZE * biome_size as f32 * biome_frequency,
        );
        let temperature_scale =
            round(settings.temperature.scale as f32 * biome_size as f32 * biome_frequency);
        // `this.seed = seed.next()`: drawn, though sampling uses the runtime seed.
        seed.next();

        let mut moisture_seed = seed.offset(settings.moisture.seed_offset);
        let moisture = CurrentMoisture {
            source: source(
                SourceKind::Simplex,
                seed_next(&mut moisture_seed),
                moisture_scale,
                1,
                0.5,
                2.0,
            ),
            power: settings.moisture.falloff,
            range: settings.moisture,
            warps: [
                noise_warp(
                    seed_next(&mut moisture_seed),
                    (moisture_scale / 2).max(1),
                    1,
                    moisture_scale as f32 / 4.0,
                ),
                noise_warp(
                    seed_next(&mut moisture_seed),
                    (moisture_scale / 6).max(1),
                    2,
                    moisture_scale as f32 / 12.0,
                ),
            ],
        };

        let mut temperature_seed = seed.offset(settings.temperature.seed_offset);
        let temperature = CurrentTemperature {
            frequency: 1.0 / temperature_scale as f32,
            power: settings.temperature.falloff,
            range: settings.temperature,
            warps: [
                noise_warp(
                    seed_next(&mut temperature_seed),
                    temperature_scale.wrapping_mul(4),
                    2,
                    temperature_scale.wrapping_mul(4) as f32,
                ),
                noise_warp(
                    seed_next(&mut temperature_seed),
                    temperature_scale,
                    1,
                    temperature_scale as f32,
                ),
            ],
        };

        let warp = CurrentWarp::new(
            axis(
                SourceKind::Simplex2,
                seed_next(seed),
                settings.biome_shape.biome_warp_scale,
                3,
                0.3,
                2.4,
                0.0,
            ),
            axis(
                SourceKind::Simplex2,
                seed_next(seed),
                settings.biome_shape.biome_warp_scale,
                3,
                0.3,
                2.4,
                0.0,
            ),
            settings.biome_shape.biome_warp_strength as f32 * 0.75,
        );
        Self {
            frequency: biome_frequency,
            warp,
            moisture,
            temperature,
        }
    }

    pub fn sample(&self, seed: i32, x: f32, y: f32, sample: &mut ClimateSample) {
        let (px, py) = self.warp.point(seed, x, y);
        self.sample_biome(seed, px * self.frequency, py * self.frequency, sample);
        sample.climate_type = BiomeType::at_climate(sample.temperature, sample.moisture);
        if matches!(
            sample.climate_type,
            BiomeType::ColdSteppe | BiomeType::Steppe
        ) {
            sample.climate_type = BiomeType::Grassland;
        }
    }

    fn sample_biome(&self, seed: i32, x: f32, y: f32, sample: &mut ClimateSample) {
        let min_x = floor(x).wrapping_sub(1);
        let min_y = floor(y).wrapping_sub(1);
        let max_x = floor(x).wrapping_add(2);
        let max_y = floor(y).wrapping_add(2);
        let mut nearest_x = x;
        let mut nearest_y = y;
        let mut nearest_hash = 0;
        let mut distance = f32::MAX;
        let mut distance2 = f32::MAX;
        for cell_y in min_y..=max_y {
            for cell_x in min_x..=max_x {
                let hash = hash_2d(seed, cell_x, cell_y);
                let point_x = CellShape::Square.cell_x(hash, cell_x, cell_y, JITTER);
                let point_y = CellShape::Square.cell_y(hash, cell_x, cell_y, JITTER);
                let dx = x - point_x;
                let dy = y - point_y;
                let candidate = dx * dx + dy * dy;
                if candidate < distance {
                    distance2 = distance;
                    distance = candidate;
                    nearest_x = point_x;
                    nearest_y = point_y;
                    nearest_hash = hash;
                } else if candidate < distance2 {
                    distance2 = candidate;
                }
            }
        }
        sample.biome_noise = random(hash(nearest_hash, 1_236_785));
        sample.biome_edge_noise = 1.0 - (f64::from(distance / distance2).sqrt() as f32);
        sample.moisture = self.moisture.sample(seed, nearest_x, nearest_y);
        sample.temperature = self.temperature.sample(seed, nearest_x, nearest_y);
    }
}

impl CurrentMoisture {
    fn sample(&self, seed: i32, x: f32, y: f32) -> f32 {
        let (x, y) = warped(&self.warps, seed, x, y);
        let value = self.source.value_seed_first(seed, x * 0.5, y);
        let mut value = (value.clamp(0.125, 0.875) - 0.125) / 0.75;
        if self.power >= 2 {
            value = (value - 0.5) * 2.0;
            value = java_pow(value, self.power).copysign(value);
            value = (value + 1.0) / 2.0;
        }
        apply_range(value, self.range)
    }
}

impl CurrentTemperature {
    fn sample(&self, seed: i32, x: f32, y: f32) -> f32 {
        let (_, mut y) = warped(&self.warps, seed, x, y);
        y *= self.frequency;
        let sin = sin_lookup(y).clamp(-1.0, 1.0);
        let value = java_pow(sin, self.power).copysign(sin);
        apply_range((value + 1.0) / 2.0, self.range)
    }
}

fn warped(warps: &[CurrentWarp; 2], seed: i32, x: f32, y: f32) -> (f32, f32) {
    let (x, y) = warps[1].point(seed, x, y);
    warps[0].point(seed, x, y)
}

fn apply_range(value: f32, range: ClimateRange) -> f32 {
    (value + range.bias() / 2.0).clamp(range.min(), range.max())
}

fn java_pow(value: f32, power: i32) -> f32 {
    let mut result = 1.0;
    for _ in 0..power {
        result *= value;
    }
    result
}

impl CurrentWarp {
    fn new(x: CurrentAxis, y: CurrentAxis, strength: f32) -> Self {
        Self { x, y, strength }
    }

    fn point(&self, seed: i32, x: f32, y: f32) -> (f32, f32) {
        (
            x + self.x.value(seed, x, y) * self.strength,
            y + self.y.value(seed, x, y) * self.strength,
        )
    }
}

impl CurrentAxis {
    fn new(source: Source, bias: f32) -> Self {
        Self {
            min: source.min_value() + bias,
            max: source.max_value() + bias,
            source,
            bias,
        }
    }

    fn value(&self, seed: i32, x: f32, y: f32) -> f32 {
        let value = self.source.value_seed_first(seed, x, y) + self.bias;
        if self.min == -0.5 && self.max == 0.5 {
            value
        } else {
            noise_map(value, self.min, self.max, self.max - self.min) - 0.5
        }
    }
}

fn source(
    kind: SourceKind,
    seed: i32,
    scale: i32,
    octaves: usize,
    gain: f32,
    lacunarity: f32,
) -> Source {
    Source::new(
        kind,
        SourceConfig {
            seed,
            octaves,
            gain,
            frequency: 1.0 / scale as f32,
            lacunarity,
            ..SourceConfig::default()
        },
    )
}

fn axis(
    kind: SourceKind,
    seed: i32,
    scale: i32,
    octaves: usize,
    gain: f32,
    lacunarity: f32,
    bias: f32,
) -> CurrentAxis {
    CurrentAxis::new(source(kind, seed, scale, octaves, gain, lacunarity), bias)
}

fn noise_warp(seed: i32, scale: i32, octaves: usize, strength: f32) -> CurrentWarp {
    CurrentWarp::new(
        axis(SourceKind::Perlin, seed, scale, octaves, 0.5, 2.0, 0.0),
        axis(
            SourceKind::Perlin,
            seed.wrapping_add(1),
            scale,
            octaves,
            0.5,
            2.0,
            0.0,
        ),
        strength,
    )
}

/// Engine's `Seed.next()`: post-increment.
fn seed_next(seed: &mut Seed) -> i32 {
    seed.next()
}

fn hash(seed: i32, value: i32) -> i32 {
    let mut hash = seed ^ 31_337_i32.wrapping_mul(value);
    hash = hash
        .wrapping_mul(hash)
        .wrapping_mul(hash)
        .wrapping_mul(60_493);
    (hash >> 13) ^ hash
}

fn round(value: f32) -> i32 {
    if value >= 0.0 {
        (value + 0.5) as i32
    } else {
        (value - 0.5) as i32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // From tools/terraforged/TerraForgedClimateNoiseOracle.java.
    const EXPECTED: &str = "0 0 0 1044776238 1051578720 1054095142 1057794348\n\
4\n\
0 1095237632 -1064828928 1044776238 1059660976 1054095142 1057794348\n\
4\n\
0 -1013522432 1118715904 1063971310 1061875646 1058343735 1057985947\n\
4\n\
0 1166016512 -973078528 1062709976 1058703423 1056241885 1049829261\n\
8\n\
17 0 0 1058014743 1061561172 1055700551 1057803069\n\
4\n\
17 1095237632 -1064828928 1058014743 1061105634 1055700551 1057803069\n\
4\n\
17 -1013522432 1118715904 1037098944 1058405905 1057937990 1057953692\n\
4\n\
17 1166016512 -973078528 1061532368 1020104128 1058429305 1051751210\n\
8\n\
-918273 0 0 1046097730 1048580968 1056555689 1057775741\n\
4\n\
-918273 1095237632 -1064828928 1046097730 1054842604 1056555689 1057775741\n\
4\n\
-918273 -1013522432 1118715904 1061664042 1058148434 1051463758 1057964102\n\
5\n\
-918273 1166016512 -973078528 1048095194 1042181076 1042757377 1051139959\n\
8\n";

    #[test]
    fn climate_noise_matches_terraforged_source() {
        let mut actual = String::new();
        let mut root = Seed::new(12_345);
        let noise = CurrentClimateNoise::new(&mut root, ClimateSettings::terraforged());
        for runtime_seed in [0, 17, -918_273] {
            for (x, y) in [
                (0.0_f32, 0.0_f32),
                (12.5, -4.25),
                (-301.75, 87.125),
                (4096.0, -8192.0),
            ] {
                let mut sample = ClimateSample::default();
                noise.sample(runtime_seed, x, y, &mut sample);
                actual.push_str(&format!(
                    "{runtime_seed} {} {} {} {} {} {}\n",
                    x.to_bits() as i32,
                    y.to_bits() as i32,
                    sample.biome_noise.to_bits() as i32,
                    sample.biome_edge_noise.to_bits() as i32,
                    sample.moisture.to_bits() as i32,
                    sample.temperature.to_bits() as i32,
                ));
                actual.push_str(&format!("{}\n", sample.climate_type.id()));
            }
        }
        assert_eq!(actual, EXPECTED);
    }
}
