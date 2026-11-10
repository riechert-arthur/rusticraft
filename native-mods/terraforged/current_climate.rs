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

// Ported from TerraForged 6dd607eb
// (https://github.com/TerraForged/TerraForged/tree/6dd607ebfd41a3b7274e090b1f2cf565d53d1d44):
// `src/main/java/com/terraforged/mod/worldgen/noise/climate/ClimateNoise.java`, over Engine 0.3.0's `Seed` and
// `ClimateSettings` and the climate settings of `NoiseGenerator.createContinentNoise`. MIT License, Copyright (c) 2021
// TerraForged (above).

use super::{
    terraforged::{
        Domain, Module, Source, SourceConfig, SourceKind, clamp, floor, hash, hash_2d, round,
    },
    terraforged_biome_sampler::ClimateSample,
    terraforged_climate::{ClimateRange, ClimateSettings, Moisture, Temperature, perlin_warp},
    terraforged_continent_cell::CellShape,
    terraforged_engine::{BiomeType, Seed},
    terraforged_terrain_blend_util::random,
};

/// `ClimateNoise.MOISTURE_SIZE`.
const MOISTURE_SIZE: f32 = 2.5;
/// `ClimateNoise.jitter`.
const JITTER: f32 = 0.8;
/// `ClimateNoise.cellShape`.
const CELL_SHAPE: CellShape = CellShape::Square;

/// The graphs below contain no custom module, so seed-first sampling cannot fail.
const SUPPORTED: &str = "climate noise graphs have only seed-first nodes";

/// TerraForged 6dd607eb's `ClimateNoise`: the warped, jittered biome cells of the newer source's biome sampler, with
/// their selection noise (`biomeNoise`), edge distance (`biomeEdgeNoise`), moisture, temperature and climate type.
/// Sampling is seed-first: the runtime seed reaches every cell hash and noise. This is NOT Engine's
/// [`super::terraforged_climate::ClimateModule`] (the jar's climate stage): no continent mask, no macro cell noise,
/// a 4x4 cell search (Engine's is 3x3), and other warps. It reads neither `macroNoiseSize` nor `biomeEdgeShape`
/// (TerraForged Reimagined's fork adds an edge warp and macro grouping; they are not followed).
pub struct CurrentClimateNoise {
    frequency: f32,
    warp: Domain,
    moisture: ClimateAxis<Moisture>,
    temperature: ClimateAxis<Temperature>,
}

// World generation shares one climate noise across threads.
const _: fn() = || {
    fn shared<T: Send + Sync>() {}
    shared::<CurrentClimateNoise>();
};

/// `settings.climate.<axis>.apply(core).warp(first).warp(second)`.
struct ClimateAxis<T> {
    core: T,
    range: ClimateRange,
    first: Domain,
    second: Domain,
}

impl<T> ClimateAxis<T> {
    /// The outer warp (`second`) moves the point first, then the inner one; `apply`'s bias and clamp follow the core.
    fn value(&self, seed: i32, x: f32, y: f32, core: impl FnOnce(&T, f32, f32) -> f32) -> f32 {
        let (x, y) = warp(&self.second, seed, x, y);
        let (x, y) = warp(&self.first, seed, x, y);
        // `Bias` then `Clamp` (`RangeValue.apply`), with the bounds read at construction, as Java's constants are.
        let range = self.range;
        clamp(
            core(&self.core, x, y) + range.bias() / 2.0,
            range.min(),
            range.max(),
        )
    }
}

/// `Domain.getX`/`getY`, both at the unwarped point.
fn warp(domain: &Domain, seed: i32, x: f32, y: f32) -> (f32, f32) {
    (
        domain.try_x_seed_first(seed, x, y).expect(SUPPORTED),
        domain.try_y_seed_first(seed, x, y).expect(SUPPORTED),
    )
}

impl ClimateSettings {
    /// The climate half of `NoiseGenerator.createContinentNoise`'s settings: Engine's defaults
    /// ([`ClimateSettings::default`]) with biome size 220, temperature falloff 2 and bias 0.1, moisture falloff 1 and
    /// bias -0.05. Every field `ClimateNoise` reads: temperature offset 0, scale 6, 0..0.98; moisture offset 0, scale
    /// 6, 0..1; warp scale 150, strength 80 (Engine's).
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
    /// `new ClimateNoise(seed, settings)`, drawing with Engine 0.3.0's [`Seed`] (`next()` returns `seed++`): the module
    /// seed (`this.seed`, which sampling never reads) from `seed`; three from the moisture offset seed (noise, then the
    /// two warps) and two from the temperature offset seed (its warps), both `new Seed(root + offset)` of `seed`'s
    /// root ([`Seed::offset`]); then the two biome-warp seeds from `seed`, in Java's order. `seed` ends three draws on.
    pub fn new(seed: &mut Seed, settings: ClimateSettings) -> Self {
        let biome_size = settings.biome_shape.biome_size;
        let temperature_scaler = settings.temperature.scale as f32;
        let moisture_scaler = settings.moisture.scale as f32 * MOISTURE_SIZE;
        let biome_frequency = 1.0 / biome_size as f32;
        let moisture_size = moisture_scaler * biome_size as f32;
        let temperature_size = temperature_scaler * biome_size as f32;
        let moisture_scale = round(moisture_size * biome_frequency);
        let temperature_scale = round(temperature_size * biome_frequency);
        let warp_scale = settings.biome_shape.biome_warp_scale;
        // `this.seed = seed.next()`: drawn, never read.
        seed.next();

        // The warp strengths are Java doubles (`moistScale / 4D`, `tempScale * 4`) narrowed to float by
        // `Source.constant`.
        let mut moisture_seed = seed.offset(settings.moisture.seed_offset);
        let core = Moisture::new(
            moisture_seed.next(),
            moisture_scale,
            settings.moisture.falloff,
        );
        let moisture = ClimateAxis {
            core,
            range: settings.moisture,
            first: perlin_warp(
                moisture_seed.next(),
                (moisture_scale / 2).max(1),
                1,
                f64::from(moisture_scale) / 4.0,
            ),
            second: perlin_warp(
                moisture_seed.next(),
                (moisture_scale / 6).max(1),
                2,
                f64::from(moisture_scale) / 12.0,
            ),
        };

        let mut temperature_seed = seed.offset(settings.temperature.seed_offset);
        let temperature_scale4 = temperature_scale.wrapping_mul(4);
        let temperature = ClimateAxis {
            core: Temperature::new(1.0 / temperature_scale as f32, settings.temperature.falloff),
            range: settings.temperature,
            first: perlin_warp(
                temperature_seed.next(),
                temperature_scale4,
                2,
                f64::from(temperature_scale4),
            ),
            second: perlin_warp(
                temperature_seed.next(),
                temperature_scale,
                1,
                f64::from(temperature_scale),
            ),
        };

        // `Source.build(seed.next(), warpScale, 3).lacunarity(2.4).gain(0.3).simplex2()`, twice.
        let warp_axis = |seed| {
            Module::source(Source::new(
                SourceKind::Simplex2,
                SourceConfig {
                    seed,
                    octaves: 3,
                    gain: 0.3,
                    frequency: 1.0 / warp_scale as f32,
                    lacunarity: 2.4,
                    ..SourceConfig::default()
                },
            ))
        };
        let warp_x = warp_axis(seed.next());
        let warp_y = warp_axis(seed.next());
        let strength = f64::from(settings.biome_shape.biome_warp_strength) * 0.75;
        let warp = Domain::domain_warp(warp_x, warp_y, Module::constant(strength as f32));

        Self {
            frequency: biome_frequency,
            warp,
            moisture,
            temperature,
        }
    }

    /// `sample(seed, x, y, sample)`: the biome warp at the runtime `seed`; the cell search at `frequency`; then the
    /// climate type from temperature and moisture (Engine 0.3.0's `BiomeType.get`, [`BiomeType::at_climate`]), with
    /// Java's temporary (`// TODO: Remove`) cold steppe and steppe to grassland swap. Writes only the five climate
    /// fields. Panics where `BiomeType.get` throws: only NaN climate bounds let a value leave 0..1 (Engine's
    /// `RangeValue` clamps the others into it).
    pub fn sample(&self, seed: i32, x: f32, y: f32, sample: &mut ClimateSample) {
        let (mut px, mut py) = warp(&self.warp, seed, x, y);
        px *= self.frequency;
        py *= self.frequency;
        px = CELL_SHAPE.adjust_x(px);
        py = CELL_SHAPE.adjust_y(py);
        self.sample_biome(seed, px, py, sample);
        sample.climate_type = BiomeType::at_climate(sample.temperature, sample.moisture);
        if matches!(
            sample.climate_type,
            BiomeType::ColdSteppe | BiomeType::Steppe
        ) {
            sample.climate_type = BiomeType::Grassland;
        }
    }

    /// Not Java's (Horizon's `glacial_landforms`): the temperature field [`Self::sample`] reads, taken at the point
    /// itself rather than at its biome cell's centre, so it varies smoothly where the biome's temperature steps from
    /// cell to cell. Same warp, frequency and cell-shape adjustment as `sample`; `x`, `y` as there.
    pub fn local_temperature(&self, seed: i32, x: f32, y: f32) -> f32 {
        let (mut px, mut py) = warp(&self.warp, seed, x, y);
        px *= self.frequency;
        py *= self.frequency;
        px = CELL_SHAPE.adjust_x(px);
        py = CELL_SHAPE.adjust_y(py);
        self.temperature
            .value(seed, px, py, |core, x, y| core.value_current(x, y))
    }

    /// `sampleBiome`: the 4x4 cells `floor - 1 ..= floor + 2` (Java `int` math; the window is empty where the upper
    /// bound wraps, as Java's loop is; `floor` never returns the `Integer.MAX_VALUE - 2` that would make Java loop
    /// forever), nearest and second-nearest by strict `<` (first wins ties, NaN never wins).
    fn sample_biome(&self, seed: i32, x: f32, y: f32, sample: &mut ClimateSample) {
        let min_x = floor(x).wrapping_sub(1);
        let min_y = floor(y).wrapping_sub(1);
        let max_x = floor(x).wrapping_add(2);
        let max_y = floor(y).wrapping_add(2);
        let (mut nearest_x, mut nearest_y) = (x, y);
        let mut nearest_hash = 0;
        let mut distance = f32::MAX;
        let mut distance2 = f32::MAX;
        for cell_y in min_y..=max_y {
            for cell_x in min_x..=max_x {
                let hash = hash_2d(seed, cell_x, cell_y);
                let px = CELL_SHAPE.cell_x(hash, cell_x, cell_y, JITTER);
                let py = CELL_SHAPE.cell_y(hash, cell_x, cell_y, JITTER);
                // `NoiseUtil.dist2`.
                let (dx, dy) = (x - px, y - py);
                let d2 = dx * dx + dy * dy;
                if d2 < distance {
                    distance2 = distance;
                    distance = d2;
                    (nearest_x, nearest_y) = (px, py);
                    nearest_hash = hash;
                } else if d2 < distance2 {
                    distance2 = d2;
                }
            }
        }
        // `MathUtil.rand(hash, 1236785)` = `rand(NoiseUtil.hash(hash, 1236785))`; its map (range 2) is the same in
        // both Noise2D versions.
        sample.biome_noise = random(hash(nearest_hash, 1_236_785));
        // `NoiseUtil.sqrt`: `(float) Math.sqrt(double)`.
        sample.biome_edge_noise = 1.0 - (f64::from(distance / distance2).sqrt() as f32);
        sample.moisture = self
            .moisture
            .value(seed, nearest_x, nearest_y, |core, x, y| {
                core.try_value_seed_first(seed, x, y).expect(SUPPORTED)
            });
        sample.temperature = self
            .temperature
            .value(seed, nearest_x, nearest_y, |core, x, y| {
                core.value_current(x, y)
            });
    }
}

#[cfg(test)]
mod tests {
    use super::super::{
        terraforged_climate::{BiomeNoise, BiomeShape},
        terraforged_engine::TerrainRegistry,
    };
    use super::*;
    use std::panic::{AssertUnwindSafe, catch_unwind};

    /// `ClimateNoiseOracle.java` (`.local/terraforged/oracles/climate_noise/`, `run.sh`): TerraForged 6dd607eb's own
    /// `ClimateNoise` compiled from source over Engine 0.3.0 (its `Seed`, `Settings`, `BiomeType`), Noise2D 2cb4843 and
    /// Engine's seed-first `Moisture`/`Temperature`, interpreted and with `-Xcomp`. Per variant: the root seed's next
    /// draw after the constructor, the samples that threw (`BiomeType.get` out of range) and an FNV-1a fold of every
    /// sample's four float bits and climate type (a throwing sample folds `0xffffffff` alone); then a fold of
    /// `BiomeType.get` over 0..1.
    const EXPECTED: [&str; 17] = [
        "v 0 18749567 0 c405b49a305c010c",
        "v 1 18749563 0 5142d907151e855a",
        "v 2 12348 0 0f389c48f8e983ac",
        "v 3 18749563 0 1e5b0a12fb0a0c99",
        "v 4 18749567 0 2ff4441d5765d424",
        "v 5 12348 0 416347f9d8ea4661",
        "v 6 -1 0 123460240c76d771",
        "v 7 3 0 a0baf4d3e89c24ba",
        "v 8 -2147483644 0 24d858a6eb03458d",
        "v 9 -2147483644 0 6a2008e374dccea5",
        "v 10 987654324 0 8ec8a2ca83895ddc",
        "v 11 -123456781 0 8f41f942ea348339",
        "v 12 45 0 d0458c7acd45e04b",
        "v 13 18749570 0 56059e455e15de05",
        "v 14 31340 0 f1b810c13e84240c",
        "v 15 559 1050 284ff75dce96db34",
        "t 91aa8d6709fe4e12",
    ];

    /// root, pre-draws, biome size, warp scale, warp strength, macro, edge type, edge scale, edge octaves, edge
    /// strength, temperature offset, scale, falloff, moisture offset, scale, falloff (the oracle's `INTS`, variants 3
    /// on; the macro and edge fields are set to show that 6dd607eb ignores them).
    const INTS: [[i32; 16]; 13] = [
        [
            18749560, 0, 220, 150, 80, 0, 0, 24, 2, 24, 0, 7, 2, 8461, 6, 1,
        ],
        [
            18749560, 4, 220, 150, 80, 0, 0, 24, 2, 0, 0, 7, 2, 8461, 6, 1,
        ],
        [12345, 0, 220, 150, 80, 4, 0, 24, 2, 24, 0, 7, 2, 8461, 6, 1],
        [-7, 3, 220, 150, 80, 1, 1, 24, 2, 24, 0, 7, 2, 8461, 6, 1],
        [0, 0, 50, 1, 1, 3, 2, 1, 1, 1, 0, 1, 1, 0, 1, 1],
        [
            i32::MAX,
            2,
            2000,
            500,
            500,
            20,
            3,
            500,
            5,
            500,
            0,
            20,
            10,
            8461,
            20,
            10,
        ],
        [
            i32::MIN,
            1,
            777,
            33,
            7,
            8,
            4,
            13,
            3,
            100,
            i32::MAX,
            3,
            3,
            i32::MIN,
            2,
            2,
        ],
        [
            987654321, 0, 220, 150, 80, 2, 5, 24, 2, 24, -5, 7, 4, 5, 6, 3,
        ],
        [
            -123456789, 5, 300, 77, 120, 6, 6, 40, 4, 60, 1, 9, 5, 2, 4, 4,
        ],
        [42, 0, 123, 9, 3, 0, 7, 7, 2, 5, 0, 5, 2, 8461, 3, 2],
        [
            18749560, 7, 220, 150, 80, 0, 0, 24, 2, 24, 0, 7, 2, 8461, 6, 1,
        ],
        [31337, 0, 64, 300, 250, 5, 0, 24, 2, 24, 0, 7, 0, 8461, 6, 0],
        [555, 1, 220, 150, 80, 0, 0, 24, 2, 24, 0, 7, 2, 8461, 6, 1],
    ];
    /// Edge gain, lacunarity; temperature min, max, bias; moisture min, max, bias (the oracle's `FLOATS`).
    const FLOATS: [[f32; 8]; 13] = [
        [0.5, 2.65, 0.0, 0.98, 0.1, 0.0, 1.0, -0.05],
        [0.5, 2.65, 0.0, 0.98, 0.1, 0.0, 1.0, -0.05],
        [0.5, 2.65, 0.0, 0.98, 0.1, 0.0, 1.0, -0.05],
        [0.5, 2.65, 0.0, 0.98, 0.1, 0.0, 1.0, -0.05],
        [0.0, 1.0, 0.0, 1.0, -1.0, 0.0, 1.0, 1.0],
        [1.0, 5.0, 0.2, 0.8, 1.0, 0.3, 0.7, -1.0],
        [0.25, 3.5, 0.9, 0.1, 0.5, 1.0, 0.0, -0.5],
        [0.5, 2.65, -0.5, 1.5, 3.0, 0.25, 0.75, -3.0],
        [0.7, 1.5, 0.4, 0.4, 0.0, 0.0, 0.01, 0.2],
        [0.4, 2.2, 0.0, 0.98, 0.1, 0.0, 1.0, -0.05],
        [0.5, 2.65, 0.0, 0.98, 0.1, 0.0, 1.0, -0.05],
        [0.5, 2.65, 0.1, 0.9, -0.3, 0.05, 0.95, 0.4],
        // NaN bounds pass every value through the range's clamp, so some leave 0..1 and `BiomeType.get` throws.
        [0.5, 2.65, f32::NAN, f32::NAN, 1.0, f32::NAN, 0.5, -1.0],
    ];
    /// Noise2D's `Source` entries the oracle uses, in its order.
    const EDGE_TYPES: [SourceKind; 8] = [
        SourceKind::Simplex,
        SourceKind::Simplex2,
        SourceKind::Perlin,
        SourceKind::Perlin2,
        SourceKind::Billow,
        SourceKind::Ridge,
        SourceKind::Cubic,
        SourceKind::SimplexRidge,
    ];
    const SEEDS: [i32; 8] = [0, 1, -1, 17, -918_273, i32::MAX, i32::MIN, 123_456_789];
    const SPECIAL: [(f32, f32); 12] = [
        (0.0, 0.0),
        (-0.0, -0.0),
        (12.5, -4.25),
        (-301.75, 87.125),
        (4096.0, -8192.0),
        (1e6, -3e7),
        (16_777_216.0, -16_777_217.0),
        (1e10, -1e10),
        (f32::INFINITY, 5.0),
        (5.0, f32::NEG_INFINITY),
        (f32::NAN, 3.0),
        (f32::from_bits(1), -f32::from_bits(1)),
    ];

    fn fold(hash: u64, value: i32) -> u64 {
        (hash ^ u64::from(value as u32)).wrapping_mul(0x0000_0100_0000_01b3)
    }

    /// The oracle's `settings(k)`: every climate field set, on top of Engine's defaults.
    fn settings(k: usize) -> ClimateSettings {
        let (i, f) = (INTS[k], FLOATS[k]);
        let mut settings = ClimateSettings::default();
        settings.temperature = ClimateRange {
            seed_offset: i[10],
            ..ClimateRange::new(i[11], i[12], f[2], f[3], f[4])
        };
        settings.moisture = ClimateRange {
            seed_offset: i[13],
            ..ClimateRange::new(i[14], i[15], f[5], f[6], f[7])
        };
        settings.biome_shape = BiomeShape {
            biome_size: i[2],
            biome_warp_scale: i[3],
            biome_warp_strength: i[4],
            macro_noise_size: i[5],
        };
        settings.biome_edge_shape = BiomeNoise {
            kind: EDGE_TYPES[i[6] as usize],
            scale: i[7],
            octaves: i[8],
            gain: f[0],
            lacunarity: f[1],
            strength: i[9],
        };
        settings
    }

    /// Variants 0 to 2: `createContinentNoise`'s settings after the continent's four draws, the same without, and
    /// Engine's plain defaults; then the oracle's `INTS`/`FLOATS` rows.
    fn variant(v: usize) -> (Seed, ClimateSettings) {
        match v {
            0 | 1 => {
                let mut seed = Seed::new(18_749_560);
                if v == 0 {
                    for _ in 0..4 {
                        seed.next();
                    }
                }
                (seed, ClimateSettings::terraforged())
            }
            2 => (Seed::new(12_345), ClimateSettings::default()),
            _ => {
                let k = v - 3;
                let mut seed = Seed::new(INTS[k][0]);
                for _ in 0..INTS[k][1] {
                    seed.next();
                }
                (seed, settings(k))
            }
        }
    }

    fn positions() -> Vec<(f32, f32)> {
        let mut positions = SPECIAL.to_vec();
        for i in -6..=6 {
            for j in -6..=6 {
                positions.push((i as f32 * 977.3 + 0.25, j as f32 * 1311.7 - 0.5));
            }
        }
        positions
    }

    #[test]
    fn climate_noise_matches_6dd607eb_java() {
        let terrains = TerrainRegistry::new();
        let positions = positions();
        let mut actual = Vec::new();
        for v in 0..3 + INTS.len() {
            let (mut seed, settings) = variant(v);
            let noise = CurrentClimateNoise::new(&mut seed, settings);
            let draws = seed.next();
            let mut hash = 0xcbf2_9ce4_8422_2325;
            let mut thrown = 0;
            for runtime_seed in SEEDS {
                for &(x, y) in &positions {
                    let mut sample = ClimateSample::new(&terrains);
                    // Java's `BiomeType.get` throws where `at_climate` panics.
                    if catch_unwind(AssertUnwindSafe(|| {
                        noise.sample(runtime_seed, x, y, &mut sample)
                    }))
                    .is_err()
                    {
                        thrown += 1;
                        hash = fold(hash, -1);
                        continue;
                    }
                    for value in [
                        sample.biome_noise.to_bits() as i32,
                        sample.biome_edge_noise.to_bits() as i32,
                        sample.moisture.to_bits() as i32,
                        sample.temperature.to_bits() as i32,
                        sample.climate_type as i32,
                    ] {
                        hash = fold(hash, value);
                    }
                }
            }
            actual.push(format!("v {v} {draws} {thrown} {hash:016x}"));
        }
        let mut hash = 0xcbf2_9ce4_8422_2325;
        for t in 0..=255 {
            for m in 0..=255 {
                hash = fold(
                    hash,
                    BiomeType::at_climate(t as f32 / 255.0, m as f32 / 255.0) as i32,
                );
            }
        }
        actual.push(format!("t {hash:016x}"));
        assert_eq!(actual, EXPECTED);
    }

    /// The oracle's `e` lines: degenerate biome sizes over Engine's defaults (root 5, runtime seed 3). Size 0 gives
    /// an infinite frequency and NaN moisture (of either sign, raw bits compared); negative sizes mirror the cells.
    const DEGENERATE: &str = "\
e 0 3f7e87ec 00000000 ffc00000 3f066666 2\n\
e 0 3f7e87ec 00000000 ffc00000 3f066666 2\n\
e 0 3f7e87ec 00000000 ffc00000 3f066666 2\n\
e 0 3f7e87ec 00000000 7fc00000 3f066666 2\n\
e -1 3e4ab01a 3efcac76 3f064964 3f0896d7 4\n\
e -1 3e73aab0 3e270de8 3e2d249b 3f02c1e5 5\n\
e -1 3d252ba0 3e1dcf38 3ed16c3d 3e020c84 9\n\
e -1 3f7e87ec 00000000 7fc00000 3f066666 2\n\
e -220 3e4ab01a 3efcac76 3f064964 3f0896d7 4\n\
e -220 3e4ab01a 3e71f7d0 3f064964 3f0896d7 4\n\
e -220 3d22dd80 3f353a3f 3f689960 3f0ab94f 4\n\
e -220 3f7e87ec 00000000 7fc00000 3f066666 2\n\
e 1 3e4ab01a 3efcac76 3f064964 3f0896d7 4\n\
e 1 3dcc19bc 3e0de650 3f6e6a5b 3f0e6100 3\n\
e 1 3edad733 3e251e10 3f1ddb48 3f4bcb0f 4\n\
e 1 3f7e87ec 00000000 7fc00000 3f066666 2\n";

    #[test]
    fn degenerate_biome_sizes_match_6dd607eb_java() {
        let terrains = TerrainRegistry::new();
        let mut actual = String::new();
        for size in [0, -1, -220, 1] {
            let mut settings = ClimateSettings::default();
            settings.biome_shape.biome_size = size;
            let noise = CurrentClimateNoise::new(&mut Seed::new(5), settings);
            for (x, y) in [
                (0.0, 0.0),
                (12.5, -4.25),
                (-3000.0, 7000.0),
                (f32::NAN, 1.0),
            ] {
                let mut sample = ClimateSample::new(&terrains);
                noise.sample(3, x, y, &mut sample);
                actual.push_str(&format!(
                    "e {size} {:08x} {:08x} {:08x} {:08x} {}\n",
                    sample.biome_noise.to_bits(),
                    sample.biome_edge_noise.to_bits(),
                    sample.moisture.to_bits(),
                    sample.temperature.to_bits(),
                    sample.climate_type as i32,
                ));
            }
        }
        assert_eq!(actual, DEGENERATE);
    }

    /// `createContinentNoise`'s climate: Engine's defaults with five fields changed.
    #[test]
    fn terraforged_settings_match_create_continent_noise() {
        let settings = ClimateSettings::terraforged();
        let r = |r: ClimateRange| (r.seed_offset, r.scale, r.falloff, r.min, r.max, r.bias);
        assert_eq!(r(settings.temperature), (0, 6, 2, 0.0, 0.98, 0.1));
        assert_eq!(r(settings.moisture), (0, 6, 1, 0.0, 1.0, -0.05));
        assert_eq!(
            settings.biome_shape,
            BiomeShape {
                biome_size: 220,
                ..BiomeShape::default()
            }
        );
    }
}
