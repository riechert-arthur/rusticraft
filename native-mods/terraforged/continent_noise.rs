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
    terraforged::{Source, SourceConfig, SourceKind},
    terraforged_context::GeneratorContext,
    terraforged_continent_carver::RiverCarver,
    terraforged_continent_cells::CellGenerator,
    terraforged_continent_config::ContinentConfig,
    terraforged_continent_points::terrain_type,
    terraforged_continent_shape::ShapeGenerator,
    terraforged_engine::ControlPoints,
    terraforged_river_network::RiverNetwork,
    terraforged_terrain_data::NoiseSample,
    terraforged_terrain_levels::TerrainLevels,
};
use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

/// How many runtime seeds' spawn offsets [`ContinentNoise`] keeps before it starts over (a world uses one).
const OFFSET_CACHE_SIZE: usize = 64;

/// The newer source tree's `ContinentNoise` (6dd607eb, `worldgen.noise.continent`): the continent module. It scales
/// world coordinates by `1 / continentScale`, warps them (two seed-first Perlin2 axes, mapped to -0.5..0.5, times
/// 0.2), adds the spawn offset, then samples the cell shapes ([`ShapeGenerator::sample_current`]) or the rivers
/// ([`RiverNetwork::sample`]). Terrains come from the context's registry (`context.terrain`), so they compare
/// `is_same` with the world's built-ins.
///
/// The spawn offset: the 1.18.2-0.3.1 jar computes `getWorldOffset()` once, in its constructor, and keeps it in a
/// field. The newer source calls `getWorldOffset(seed)` on every sample, which computes the first caller's offset and
/// then returns it for every runtime seed. Here each runtime seed gets its own offset, computed once on its first
/// sample and kept (up to [`OFFSET_CACHE_SIZE`] seeds), so two runtime seeds never share one; for a world sampled
/// with one seed, as TerraForged does, this is the source's result.
pub struct ContinentNoise {
    context: GeneratorContext,
    control_points: ControlPoints,
    cells: Arc<CellGenerator>,
    shape: ShapeGenerator,
    rivers: RiverNetwork<Arc<CellGenerator>>,
    carver: RiverCarver,
    /// `warp`'s axes (`Domain.warp(x, y, Source.constant(0.2))`).
    warp_x: Source,
    warp_y: Source,
    frequency: f32,
    offsets: RwLock<HashMap<i32, (f32, f32)>>,
}

impl ContinentNoise {
    /// `new ContinentNoise(levels, context)`: draws `seed0`, `seed1` (`createContinent`), then the two warp seeds from
    /// `context.seed`, in that order.
    pub fn new(levels: TerrainLevels, mut context: GeneratorContext) -> Self {
        let control_points = ControlPoints::new(context.settings.world.control_points);
        let scale = context.settings.world.continent.continent_scale;
        let mut config = ContinentConfig::default();
        config.shape.scale = scale;
        config.shape.seed0 = context.seed.next();
        config.shape.seed1 = context.seed.next();
        let cells = Arc::new(CellGenerator::new(config));
        let carver = RiverCarver::new(levels.noise_levels, config);
        let shape = ShapeGenerator::new(config, control_points);
        let rivers = RiverNetwork::new(Arc::clone(&cells), &shape, config);
        // `1F / continentScale`: the int scale converted to float, then a float division.
        let frequency = 1.0 / scale as f32;
        let warp = |seed| {
            let config = SourceConfig {
                seed,
                octaves: 3,
                lacunarity: 2.2,
                frequency: 3.0,
                gain: 0.3,
                ..SourceConfig::default()
            };
            Source::new(SourceKind::Perlin2, config)
        };
        let warp_x = warp(context.seed.next());
        let warp_y = warp(context.seed.next());
        Self {
            context,
            control_points,
            cells,
            shape,
            rivers,
            carver,
            warp_x,
            warp_y,
            frequency,
            offsets: RwLock::new(HashMap::new()),
        }
    }

    /// `getContext`.
    pub fn context(&self) -> &GeneratorContext {
        &self.context
    }

    /// `getControlPoints`.
    pub fn control_points(&self) -> ControlPoints {
        self.control_points
    }

    /// `sampleContinent(seed, x, y, sample)`: sets `continent_noise`, `base_noise` and `terrain_type` (a deep or
    /// shallow ocean, a coast or none, by `ContinentPoints.getTerrainType`).
    pub fn sample_continent(&self, seed: i32, x: f32, y: f32, sample: &mut NoiseSample) {
        let (x, y) = self.position(seed, x, y);
        self.shape.sample_current(&self.cells, seed, x, y, sample);
        sample.terrain_type = terrain_type(sample.continent_noise, &self.context.terrain);
    }

    /// `sampleRiver(seed, x, y, sample)`: carves the rivers and lakes into a sample that [`Self::sample_continent`]
    /// filled (and whose height noise the caller set).
    pub fn sample_river(&self, seed: i32, x: f32, y: f32, sample: &mut NoiseSample) {
        let (x, y) = self.position(seed, x, y);
        self.rivers
            .sample(seed, x, y, sample, &self.carver, &self.context.terrain);
    }

    /// The spawn offset for `seed` (`getWorldOffset(seed)`, see [`ContinentNoise`]): the first call per seed runs
    /// the spiral search ([`CellGenerator::world_offset`]) outside the lock; concurrent first calls may both run it
    /// and agree.
    pub fn world_offset(&self, seed: i32) -> (f32, f32) {
        let read = self
            .offsets
            .read()
            .unwrap_or_else(|poison| poison.into_inner());
        if let Some(&offset) = read.get(&seed) {
            return offset;
        }
        drop(read);
        let offset = self.cells.world_offset(seed);
        let mut offsets = self
            .offsets
            .write()
            .unwrap_or_else(|poison| poison.into_inner());
        if offsets.len() >= OFFSET_CACHE_SIZE {
            offsets.clear();
        }
        *offsets.entry(seed).or_insert(offset)
    }

    /// Both samplers' `x *= frequency; px = warp.getX(seed, x, y) + offset.x` (and `y`). `DomainWarp` maps each
    /// Perlin2 axis (bounds 0..1) by `Map`: `-0.5 + (v - 0) / 1 * (0.5 - -0.5)`, which is `-0.5 + v` bit for bit.
    fn position(&self, seed: i32, x: f32, y: f32) -> (f32, f32) {
        let x = x * self.frequency;
        let y = y * self.frequency;
        let warped_x = x + (-0.5 + self.warp_x.value_seed_first(seed, x, y)) * 0.2;
        let warped_y = y + (-0.5 + self.warp_y.value_seed_first(seed, x, y)) * 0.2;
        let (offset_x, offset_y) = self.world_offset(seed);
        (warped_x + offset_x, warped_y + offset_y)
    }
}

#[cfg(test)]
mod tests {
    use super::super::terraforged_context::GeneratorSettings;
    use super::super::terraforged_engine::WorldControlPoints;
    use super::*;

    // `ContinentNoiseOracle.java` (`.local/terraforged/oracles/continent_noise/`, `run.sh`): the newer source's
    // ContinentNoise compiled against Noise2D 2cb4843, caches bypassed; its runtime-seed-0 lines equal the jar's
    // except config 2 (inland 1.0, where the jar's `NoiseUtil.map` gives NaN). Configs: world seed, continent scale,
    // control points (deep ocean up to inland; `None` = defaults; config 3's are invalid and fall back), levels.
    const MIN: f32 = f32::from_bits(1);
    const RUNTIME: [i32; 6] = [0, 42, -17, i32::MAX, i32::MIN, 0x5eed_1234];
    #[rustfmt::skip]
    const POINTS: [f32; 26] = [f32::NAN, f32::NEG_INFINITY, f32::INFINITY, -0.0, 0.0, MIN, -MIN, 1e30, -1e30,
        f32::MAX, -f32::MAX, 2_147_483_520.0, 2_147_483_648.0, -2_147_483_648.0, -2_147_483_520.0, 16_777_216.0,
        -16_777_216.0, 16_777_218.0, 0.5, -0.5, 1.75, -2.25, 123.4, -98.76, 0.999_999_94, -1.0];
    #[rustfmt::skip]
    const CONFIGS: [(i64, i32, Option<[f32; 5]>, usize); 6] = [
        (18_749_560, 400, Some([0.05, 0.3, 0.45, 0.75, 0.8]), 0),
        (18_749_560, 3000, None, 1),
        (-5, 1000, Some([0.1, 0.25, 0.327, 0.448, 1.0]), 0),
        (0x0123_4567_89ab_cdef, 1, Some([0.5, 0.4, 0.3, 0.2, 0.1]), 1),
        (7, -400, Some([0.0, 0.2, 0.4, 0.6, 0.99]), 0),
        (99, 0, None, 1),
    ];

    /// Per config: `seed.get()` after the constructor, the grid hash and spawn offset bits per runtime seed (in
    /// [`RUNTIME`] order), the edge points' hash (runtime seed 0) and `fresh` (every grid in one fold).
    struct Expected {
        draws: i32,
        grid: [u64; 6],
        offsets: [(u32, u32); 6],
        edges: u64,
        fresh: u64,
    }

    #[rustfmt::skip]
    const EXPECTED: [Expected; 6] = [
        Expected {
            draws: 18749564,
            grid: [0x795b_53d3_5fc9_a19f, 0x5f2a_80f4_bd50_2f9d, 0xac6a_0809_2471_0fbb, 0x5e1b_5ec3_512c_9e4d, 0x6cbf_f6db_c768_9260, 0x8e75_9cc2_ba3e_64eb],
            offsets: [(0x4021_b8af, 0x3f33_b16c), (0x4047_b2a4, 0xc026_122b), (0xc02e_e5ef, 0x4043_0268), (0xbf24_3040, 0x40b6_6c6c), (0x4015_8358, 0xbe83_293a), (0x3f86_ef73, 0x3e63_cac2)],
            edges: 0x17e6_824e_a442_da72,
            fresh: 0x0ff7_9ed3_cbeb_d46a,
        },
        Expected {
            draws: 18749564,
            grid: [0x7394_1ef3_de38_136f, 0x4b16_9c1b_600d_c6c7, 0x450d_0780_e10c_d93b, 0x41b7_c980_71b0_87eb, 0x663e_c0dc_3d54_4946, 0xd955_3e2b_f3cb_177d],
            offsets: [(0x4021_b8af, 0x3f33_b16c), (0x4047_b2a4, 0xc026_122b), (0xc02e_e5ef, 0x4043_0268), (0xbf24_3040, 0x40b6_6c6c), (0x4015_8358, 0xbe83_293a), (0x3f86_ef73, 0x3e63_cac2)],
            edges: 0x43c7_42ee_3c09_4160,
            fresh: 0x3890_b2dd_0ae0_4ef2,
        },
        Expected {
            draws: -1,
            grid: [0x619f_0c2a_eebb_6d66, 0x21a2_d9a2_cd20_7e17, 0x0fa3_91f7_74bc_473b, 0x810d_5f5c_8aa1_8661, 0xd07d_7122_db65_5887, 0xe5bd_98b4_916b_fd9b],
            offsets: [(0x4020_7f1d, 0xc060_9123), (0x401d_09e0, 0xbf54_f768), (0x3ef5_7e02, 0x4024_ba02), (0x3f97_9e4c, 0xc01e_f9dd), (0xc03c_5cc0, 0x4016_2a6b), (0xc039_f3f9, 0xc038_5d1c)],
            edges: 0x1cf2_a34c_d4e4_8a29,
            fresh: 0xbc79_0a17_1bca_4426,
        },
        Expected {
            draws: -1985229325,
            grid: [0x5961_ef05_cd6a_40d0, 0xbb82_841f_7b2a_57a4, 0x158a_3cb1_f6cc_b4d0, 0xb64f_6a67_82b9_7393, 0xa89c_4637_effa_a9db, 0xab1b_f6fd_9ba3_4765],
            offsets: [(0x40a7_7df5, 0xbfb9_1a1c), (0x3fd8_ed76, 0xc03b_849c), (0xbf25_2618, 0x3f80_23a5), (0x3f80_5044, 0x3e81_edb4), (0xc0fa_51d0, 0x40ee_452a), (0xc01d_1a13, 0xc03f_9aed)],
            edges: 0x9420_ef47_f57e_9db4,
            fresh: 0x4204_9f3d_ff9b_4938,
        },
        Expected {
            draws: 11,
            grid: [0x0120_dbc6_3fc0_2e78, 0x9f76_d24b_239f_15d7, 0x1936_5cb6_7ca6_45ae, 0x830c_fb49_4026_457a, 0x12e2_90b2_ddb0_91d0, 0x595e_0688_a643_9106],
            offsets: [(0x4025_74a7, 0xc01f_5ac5), (0xc033_1965, 0xc025_2e32), (0xbeba_ab1c, 0x3f9a_3326), (0xc021_cd44, 0xc035_8404), (0xc032_d5cb, 0x3f96_2059), (0x3fb5_48ef, 0x3f12_d7d4)],
            edges: 0x8ae4_37a0_c2be_0db1,
            fresh: 0x9a4f_d5b0_1edd_5b0c,
        },
        Expected {
            draws: 103,
            grid: [0xf28b_7ed4_6c56_04c5, 0xf28b_7ed4_6c56_04c5, 0xf28b_7ed4_6c56_04c5, 0xf28b_7ed4_6c56_04c5, 0xf28b_7ed4_6c56_04c5, 0xf28b_7ed4_6c56_04c5],
            offsets: [(0xc02c_f022, 0x4085_79fc), (0x3fc6_6d39, 0x3f2f_f0d8), (0x4022_9bb5, 0x3e1a_dd8a), (0x4002_8244, 0xc038_92ef), (0xbfa0_5bf1, 0x40b3_7cca), (0x402d_81d2, 0xbff9_b8fd)],
            edges: 0x743a_d4c5_6bc6_19a5,
            fresh: 0x79e4_b5b0_e437_d8e5,
        },
    ];

    fn levels(index: usize) -> TerrainLevels {
        if index == 0 {
            TerrainLevels::new(false, 1.0, -64, 256, 128, 62, 22)
        } else {
            TerrainLevels::new(true, 2.5, 0, 480, 200, 63, 30)
        }
    }

    fn noise(config: usize) -> ContinentNoise {
        let (seed, scale, points, level) = CONFIGS[config];
        let mut settings = GeneratorSettings::default();
        settings.world.seed = seed;
        settings.world.continent.continent_scale = scale;
        if let Some([deep_ocean, shallow_ocean, beach, coast, inland]) = points {
            settings.world.control_points = WorldControlPoints {
                deep_ocean,
                shallow_ocean,
                beach,
                coast,
                inland,
            };
        }
        ContinentNoise::new(levels(level), GeneratorContext::new(settings))
    }

    /// The oracle's FNV-1a over 64-bit values; floats as canonical `floatToIntBits`.
    struct Fold(u64);

    impl Fold {
        fn new() -> Self {
            Self(0xcbf2_9ce4_8422_2325)
        }

        fn add(&mut self, value: u64) {
            self.0 = (self.0 ^ value).wrapping_mul(0x0100_0000_01b3);
        }

        fn float(&mut self, value: f32) {
            self.add(u64::from(if value.is_nan() {
                0x7fc0_0000
            } else {
                value.to_bits()
            }));
        }

        /// The oracle's `sample`: continent, then the stand-in height `toHeightNoise(base, 0.35)`, then rivers.
        fn sample(&mut self, noise: &ContinentNoise, level: usize, seed: i32, x: f32, y: f32) {
            let mut sample = NoiseSample::new(&noise.context.terrain);
            noise.sample_continent(seed, x, y, &mut sample);
            self.float(sample.continent_noise);
            self.float(sample.base_noise);
            self.add(i64::from(sample.terrain_type.id()) as u64);
            sample.height_noise = levels(level)
                .noise_levels
                .to_height_noise(sample.base_noise, 0.35);
            noise.sample_river(seed, x, y, &mut sample);
            for value in [
                sample.continent_noise,
                sample.base_noise,
                sample.height_noise,
                sample.river_noise,
            ] {
                self.float(value);
            }
            self.add(i64::from(sample.terrain_type.id()) as u64);
        }

        /// The oracle's 11x11 grid around the origin, a little over a third of a cell apart.
        fn grid(&mut self, noise: &ContinentNoise, config: usize, seed: i32) {
            let step = CONFIGS[config].1.unsigned_abs() as f32 * 0.37 + 0.75;
            for y in -5..=5 {
                for x in -5..=5 {
                    let (x, y) = (x as f32 * step + 13.25, y as f32 * step - 7.5);
                    self.sample(noise, CONFIGS[config].3, seed, x, y);
                }
            }
        }
    }

    #[test]
    fn continent_noise_matches_newer_source() {
        for (config, expected) in EXPECTED.iter().enumerate() {
            for (index, seed) in RUNTIME.into_iter().enumerate() {
                let noise = noise(config);
                assert_eq!(noise.context().seed.get(), expected.draws, "draws {config}");
                let mut fold = Fold::new();
                fold.grid(&noise, config, seed);
                assert_eq!(fold.0, expected.grid[index], "grid {config} {seed}");
                let (x, y) = noise.world_offset(seed);
                assert_eq!(
                    (x.to_bits(), y.to_bits()),
                    expected.offsets[index],
                    "offset {config} {seed}"
                );
                if seed == 0 {
                    let mut fold = Fold::new();
                    for y in POINTS {
                        for x in POINTS {
                            fold.sample(&noise, CONFIGS[config].3, seed, x, y);
                        }
                    }
                    assert_eq!(fold.0, expected.edges, "edges {config}");
                }
            }
        }
    }

    /// One `ContinentNoise` sampled at every runtime seed in turn gives each seed its own offset: the oracle's
    /// `fresh` fold (a new one per seed). The newer source's own `shared` fold differs: it keeps the first seed's.
    #[test]
    fn runtime_seeds_never_share_an_offset() {
        for (config, expected) in EXPECTED.iter().enumerate() {
            let noise = noise(config);
            let mut fold = Fold::new();
            for seed in RUNTIME {
                fold.grid(&noise, config, seed);
            }
            assert_eq!(fold.0, expected.fresh, "shared {config}");
        }
        let noise = noise(0);
        let offsets = RUNTIME.map(|seed| noise.world_offset(seed));
        for (index, offset) in offsets.iter().enumerate() {
            assert_eq!(noise.world_offset(RUNTIME[index]), *offset);
            assert!(offsets[..index].iter().all(|other| other != offset));
        }
        // Past OFFSET_CACHE_SIZE seeds the store starts over: still each seed's own offset, and bounded.
        for seed in 0..(OFFSET_CACHE_SIZE as i32 * 2 + 3) {
            assert_eq!(
                noise.world_offset(seed),
                noise.cells.world_offset(seed),
                "{seed}"
            );
        }
        assert!(noise.offsets.read().unwrap().len() <= OFFSET_CACHE_SIZE);
    }

    /// TerraForged samples one `ContinentNoise` from its worker threads: twelve threads on one, two per runtime seed,
    /// all starting before any offset is known, still get each seed's own grid and offset.
    #[test]
    fn continent_noise_samples_the_same_from_parallel_workers() {
        let noise = noise(0);
        let expected = &EXPECTED[0];
        std::thread::scope(|scope| {
            for (index, seed) in RUNTIME.into_iter().enumerate().cycle().take(12) {
                let noise = &noise;
                scope.spawn(move || {
                    let mut fold = Fold::new();
                    fold.grid(noise, 0, seed);
                    assert_eq!(fold.0, expected.grid[index], "grid {seed}");
                    let (x, y) = noise.world_offset(seed);
                    assert_eq!((x.to_bits(), y.to_bits()), expected.offsets[index]);
                });
            }
        });
        fn shared<T: Send + Sync>() {}
        shared::<ContinentNoise>();
    }
}
