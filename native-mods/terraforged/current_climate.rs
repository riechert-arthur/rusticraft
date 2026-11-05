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

// Adapted from TerraForged Reimagined (https://github.com/Romoslayer/TerraForged-Reimagined, a24d450):
// `src/main/java/com/terraforged/mod/worldgen/noise/climate/ClimateNoise.java` (with its biome-edge warp and macro
// noise grouping, which TerraForged 6dd607eb's ClimateNoise lacks), its reconstructed Engine `Seed` (pre-increment
// draws) and `TerraSettings.Climate` defaults. MIT License, Copyright (c) 2020 TerraForged, Copyright (c) 2026
// Romoslayer; see `LICENSE-TERRAFORGED-REIMAGINED`.

use super::{
    terraforged::{
        Domain, Module, Source, SourceConfig, SourceKind, clamp, floor, hash, hash_2d, round,
    },
    terraforged_biome_sampler::ClimateSample,
    terraforged_climate::{
        BiomeNoise, BiomeShape, ClimateRange, ClimateSettings, Moisture, Temperature, perlin_warp,
    },
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

/// TerraForged Reimagined's `ClimateNoise`: the warped, jittered biome cells of the newer source's biome sampler, with
/// their selection noise (`biomeNoise`), edge distance (`biomeEdgeNoise`), moisture, temperature and climate type.
/// Sampling is seed-first: the runtime seed reaches every cell hash and noise. This is NOT Engine's
/// [`super::terraforged_climate::ClimateModule`] (the jar's climate stage): no continent mask, no macro cell noise,
/// a 4x4 cell search with second-nearest edge distance, and other warps.
pub struct CurrentClimateNoise {
    frequency: f32,
    warp: Domain,
    edge_warp: Option<Domain>,
    macro_noise_size: i32,
    moisture: ClimateAxis<Moisture>,
    temperature: ClimateAxis<Temperature>,
}

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
    /// TerraForged Reimagined's `TerraSettings.Climate` defaults, as `NoiseGenerator.createContinentNoise` copies
    /// them into the Engine settings (every field `ClimateNoise` reads): temperature offset 0, scale 7, falloff 2,
    /// 0..0.98, bias 0.1; moisture offset 8461, scale 6, falloff 1, 0..1, bias -0.05; biome size 220, warp scale 150,
    /// strength 80, macro noise off (0); edge warp simplex, scale 24, 2 octaves, gain 0.5, lacunarity 2.65,
    /// strength 24. Engine's own defaults ([`ClimateSettings::default`]) differ.
    pub fn terra_defaults() -> Self {
        Self {
            temperature: ClimateRange {
                seed_offset: 0,
                ..ClimateRange::new(7, 2, 0.0, 0.98, 0.1)
            },
            moisture: ClimateRange {
                seed_offset: 8461,
                ..ClimateRange::new(6, 1, 0.0, 1.0, -0.05)
            },
            biome_shape: BiomeShape {
                biome_size: 220,
                macro_noise_size: 0,
                biome_warp_scale: 150,
                biome_warp_strength: 80,
            },
            biome_edge_shape: BiomeNoise {
                strength: 24,
                ..BiomeNoise::default()
            },
        }
    }
}

/// Reimagined's `Seed.next()`: `++seed` (Engine 0.3.0's, which [`Seed::next`] ports, returns `seed++`). Same state
/// afterwards, the drawn value one higher. `Seed.offset` (`new Seed(value + offset)`) is [`Seed::offset`] for the
/// root seeds drawn from here.
fn next(seed: &mut Seed) -> i32 {
    seed.next().wrapping_add(1)
}

impl CurrentClimateNoise {
    /// `new ClimateNoise(seed, settings)`: draws the module seed from `seed`, two from the moisture offset seed's
    /// stream (noise, then the two warps), two from the temperature one (its warps), then the two biome-warp seeds
    /// from `seed`, in Java's order. The edge warp is seeded from fixed offsets of the module seed (7919, 104729,
    /// wrapping) and exists only for a strength above 0.
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
        let module_seed = next(seed);

        // The warp strengths are Java doubles (`moistScale / 4D`, `tempScale * 4`) narrowed to float by
        // `Source.constant`.
        let mut moisture_seed = seed.offset(settings.moisture.seed_offset);
        let core = Moisture::new(
            next(&mut moisture_seed),
            moisture_scale,
            settings.moisture.falloff,
        );
        let moisture = ClimateAxis {
            core,
            range: settings.moisture,
            first: perlin_warp(
                next(&mut moisture_seed),
                (moisture_scale / 2).max(1),
                1,
                f64::from(moisture_scale) / 4.0,
            ),
            second: perlin_warp(
                next(&mut moisture_seed),
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
                next(&mut temperature_seed),
                temperature_scale4,
                2,
                f64::from(temperature_scale4),
            ),
            second: perlin_warp(
                next(&mut temperature_seed),
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
        let warp_x = warp_axis(next(seed));
        let warp_y = warp_axis(next(seed));
        let strength = f64::from(settings.biome_shape.biome_warp_strength) * 0.75;
        let warp = Domain::domain_warp(warp_x, warp_y, Module::constant(strength as f32));

        let edge = settings.biome_edge_shape;
        let edge_warp = (edge.strength > 0).then(|| {
            Domain::domain_warp(
                edge.build(module_seed.wrapping_add(7919)),
                edge.build(module_seed.wrapping_add(104_729)),
                Module::constant(edge.strength as f32),
            )
        });

        Self {
            frequency: biome_frequency,
            warp,
            edge_warp,
            macro_noise_size: settings.biome_shape.macro_noise_size.max(0),
            moisture,
            temperature,
        }
    }

    /// `sample(seed, x, y, sample)`: the biome warp, then the edge warp (when on), both at the runtime `seed`; the
    /// cell search at `frequency`; then the climate type from temperature and moisture, with Java's temporary
    /// (`// TODO: Remove`) cold steppe and steppe to grassland swap. Writes only the five climate fields.
    pub fn sample(&self, seed: i32, x: f32, y: f32, sample: &mut ClimateSample) {
        let (mut px, mut py) = warp(&self.warp, seed, x, y);
        if let Some(edge_warp) = &self.edge_warp {
            (px, py) = warp(edge_warp, seed, px, py);
        }
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
        let (mut nearest_cell_x, mut nearest_cell_y) = (0, 0);
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
                    (nearest_cell_x, nearest_cell_y) = (cell_x, cell_y);
                } else if d2 < distance2 {
                    distance2 = d2;
                }
            }
        }
        // `Math.floorDiv` by a positive size is `div_euclid`.
        let biome_hash = if self.macro_noise_size <= 0 {
            nearest_hash
        } else {
            hash_2d(
                seed,
                nearest_cell_x.div_euclid(self.macro_noise_size),
                nearest_cell_y.div_euclid(self.macro_noise_size),
            )
        };
        // `MathUtil.rand(hash, 1236785)` = `rand(NoiseUtil.hash(hash, 1236785))`; its map (range 2) is the same in
        // both Noise2D versions.
        sample.biome_noise = random(hash(biome_hash, 1_236_785));
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
    use super::super::terraforged_engine::TerrainRegistry;
    use super::*;

    /// `CurrentClimateOracle.java` (`.local/terraforged/oracles/current_climate/`, `run.sh`): Reimagined a24d450's
    /// own `ClimateNoise`, `Seed`, `Settings`, `Moisture`, `Temperature` and `BiomeType` compiled from source,
    /// interpreted and with `-Xcomp`. Per variant: the root seed's next draw after the constructor and an FNV-1a fold
    /// of every sample's four float bits and climate type; then a fold of `BiomeType.get` over a 1025² grid.
    const EXPECTED: [&str; 13] = [
        "v 0 18749564 bc9efcf98064af2f",
        "v 1 18749568 1568f4897a4a9173",
        "v 2 12349 446990327354f5ba",
        "v 3 0 8979922364b5699c",
        "v 4 4 e2ef57355131ad56",
        "v 5 -2147483643 31514a1ffbe5ed3e",
        "v 6 -2147483643 67f0908fe201d0db",
        "v 7 987654325 a9ee586ff3265a57",
        "v 8 -123456780 3ab0d1fe96b2b393",
        "v 9 46 70e884a8e56d4f66",
        "v 10 18749571 2d8ecacf0a68e996",
        "v 11 31341 83c24cdf93ec1e38",
        "t f6c331fb517e7087",
    ];

    /// root, pre-draws, biome size, warp scale, warp strength, macro, edge type, edge scale, edge octaves, edge
    /// strength, temperature offset, scale, falloff, moisture offset, scale, falloff (the oracle's `INTS`).
    const INTS: [[i32; 16]; 12] = [
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
    ];
    /// Edge gain, lacunarity; temperature min, max, bias; moisture min, max, bias (the oracle's `FLOATS`).
    const FLOATS: [[f32; 8]; 12] = [
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
    ];
    /// Noise2D's `Source` entries the oracle uses, in its order (`SIMPLEX_RIDGE` builds a simplex2 noise).
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

    fn settings(variant: usize) -> ClimateSettings {
        let (i, f) = (INTS[variant], FLOATS[variant]);
        let mut settings = ClimateSettings::terra_defaults();
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
    fn current_climate_noise_matches_reimagined_java() {
        let terrains = TerrainRegistry::new();
        let positions = positions();
        let mut actual = Vec::new();
        for variant in 0..INTS.len() {
            let mut seed = Seed::new(INTS[variant][0]);
            for _ in 0..INTS[variant][1] {
                next(&mut seed);
            }
            let noise = CurrentClimateNoise::new(&mut seed, settings(variant));
            let draws = next(&mut seed);
            let mut hash = 0xcbf2_9ce4_8422_2325;
            for runtime_seed in SEEDS {
                for &(x, y) in &positions {
                    let mut sample = ClimateSample::new(&terrains);
                    noise.sample(runtime_seed, x, y, &mut sample);
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
            actual.push(format!("v {variant} {draws} {hash:016x}"));
        }
        let mut hash = 0xcbf2_9ce4_8422_2325;
        for t in 0..=1024 {
            for m in 0..=1024 {
                hash = fold(
                    hash,
                    BiomeType::at_climate(t as f32 / 1024.0, m as f32 / 1024.0) as i32,
                );
            }
        }
        actual.push(format!("t {hash:016x}"));
        assert_eq!(actual, EXPECTED);
    }

    /// The defaults are TerraSettings' (Engine's own differ in every axis).
    #[test]
    fn terra_defaults_match_reimagined_terra_settings() {
        let settings = ClimateSettings::terra_defaults();
        let r = |r: ClimateRange| (r.seed_offset, r.scale, r.falloff, r.min, r.max, r.bias);
        assert_eq!(r(settings.temperature), (0, 7, 2, 0.0, 0.98, 0.1));
        assert_eq!(r(settings.moisture), (8461, 6, 1, 0.0, 1.0, -0.05));
        assert_eq!(
            settings.biome_shape,
            BiomeShape {
                biome_size: 220,
                macro_noise_size: 0,
                biome_warp_scale: 150,
                biome_warp_strength: 80,
            }
        );
        let edge = settings.biome_edge_shape;
        assert!(matches!(edge.kind, SourceKind::Simplex));
        assert_eq!(
            (
                edge.scale,
                edge.octaves,
                edge.gain,
                edge.lacunarity,
                edge.strength
            ),
            (24, 2, 0.5, 2.65, 24)
        );
    }
}
