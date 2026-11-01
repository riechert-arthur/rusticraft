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
// `src/main/java/com/terraforged/mod/worldgen/Generator.java` (`createState`'s seed capture, `getSeed`,
// `getChunkData`, `getChunkDataAsync`, `getMinY`, `getSeaLevel`, `getGenDepth`, the terrain-cache hints and drop,
// `getBaseHeight`, `getBaseColumn`, and `buildTerrain`'s hand-off to `fillFromNoise`) and `Seeds.java`. MIT License, Copyright (c) 2020 TerraForged, Copyright (c)
// 2026 Romoslayer; see `LICENSE-TERRAFORGED-REIMAGINED`.

use super::{
    super::terrain::{TerrainBlockStates, TerrainChunk, TerrainError},
    terraforged_chunk_fill::fill_chunk,
    terraforged_eroded_noise_generator::ErodedNoiseGenerator,
    terraforged_terrain_cache::{TerrainCache, TerrainDataHandle, TerrainTask},
    terraforged_terrain_levels::TerrainLevels,
};
use crate::registry::BlockStateId;
use std::sync::atomic::{AtomicI32, AtomicI64, Ordering};

/// `Seeds.get(long)`: the level seed as TerraForged's noise uses it, narrowed to a Java `int` (its low 32 bits).
pub const fn seed_of(level_seed: i64) -> i32 {
    level_seed as i32
}

/// Minecraft's `Heightmap.Types`, as `getBaseHeight` splits them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TerraHeightmap {
    WorldSurface,
    WorldSurfaceWorldgen,
    MotionBlocking,
    MotionBlockingNoLeaves,
    OceanFloor,
    OceanFloorWorldgen,
}

impl TerraHeightmap {
    /// The surface types count the water above the terrain; the ocean-floor types stop at the terrain.
    const fn includes_water(self) -> bool {
        matches!(
            self,
            Self::WorldSurface
                | Self::WorldSurfaceWorldgen
                | Self::MotionBlocking
                | Self::MotionBlockingNoLeaves
        )
    }
}

/// The two block states `getBaseColumn` fills a column with.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ColumnBlock {
    Stone,
    Water,
}

/// `getBaseColumn`'s `NoiseColumn(minY, states)`. Java passes the terrain height as `minY` while `states[0]` is block
/// Y 0 (stone from index 0 up to the height, water up to the surface); the port keeps both as Java has them.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BaseColumn {
    pub min_y: i32,
    pub states: Vec<ColumnBlock>,
}

/// The queries Minecraft's `ChunkGenerator` asks Reimagined's `Generator`, over its `TerrainCache` of the eroded
/// generator. Heights count from block 0 as TerraForged's levels do (`levels.max_y` is the exclusive top, 480 by
/// default); mapping them into a dimension's build range is the job of the layer that registers the generator.
pub struct TerraForgedGenerator {
    levels: TerrainLevels,
    terrain: TerrainCache<ErodedNoiseGenerator>,
    seed: AtomicI32,
    level_seed: AtomicI64,
}

impl TerraForgedGenerator {
    /// The generator over `noise`, with a new [`TerrainCache`]. The seed is 0 until [`Self::capture_seed`] (Java's
    /// fields start at 0 too).
    pub fn new(levels: TerrainLevels, noise: ErodedNoiseGenerator) -> Self {
        Self {
            levels,
            terrain: TerrainCache::new(levels, noise),
            seed: AtomicI32::new(0),
            level_seed: AtomicI64::new(0),
        }
    }

    /// `createState`'s capture: `seed = Seeds.get(levelSeed)`, `levelSeed = levelSeed` (two volatile writes in Java;
    /// two release stores here, so a reader can briefly see one new and one old value, as in Java).
    pub fn capture_seed(&self, level_seed: i64) {
        self.seed.store(seed_of(level_seed), Ordering::Release);
        self.level_seed.store(level_seed, Ordering::Release);
    }

    /// `getSeed()`: the captured `int` seed every query below samples with.
    pub fn seed(&self) -> i32 {
        self.seed.load(Ordering::Acquire)
    }

    /// The captured raw level seed (Java's `levelSeed` field, read by the carver and Deep Caves stages).
    pub fn level_seed(&self) -> i64 {
        self.level_seed.load(Ordering::Acquire)
    }

    pub const fn levels(&self) -> TerrainLevels {
        self.levels
    }

    /// `getMinY()`: `levels.minY`.
    pub const fn min_y(&self) -> i32 {
        self.levels.min_y
    }

    /// `getSeaLevel()`: `levels.seaLevel`.
    pub const fn sea_level(&self) -> i32 {
        self.levels.sea_level
    }

    /// `getGenDepth()`: `levels.maxY`, the exclusive top block index (not `maxY - minY`), as in Java.
    pub const fn gen_depth(&self) -> i32 {
        self.levels.max_y
    }

    /// `getChunkData(seed, pos)`: the chunk's terrain data, generated (or waited for) now.
    pub fn chunk_data(
        &self,
        seed: i32,
        chunk_x: i32,
        chunk_z: i32,
    ) -> TerrainDataHandle<ErodedNoiseGenerator> {
        self.terrain.get_now(seed, chunk_x, chunk_z)
    }

    /// `getChunkDataAsync(seed, pos)`: the chunk's shared generation task.
    pub fn chunk_data_async(
        &self,
        seed: i32,
        chunk_x: i32,
        chunk_z: i32,
    ) -> TerrainTask<ErodedNoiseGenerator> {
        self.terrain.get_async(seed, chunk_x, chunk_z)
    }

    /// `terrainCache().hint(seed, pos)`: starts the chunk's generation. Java's hooks pass different seeds:
    /// `createStructures` `Seeds.get(state.getLevelSeed())`, `createReferences` `Seeds.get(level.getSeed())`,
    /// `createBiomes` the captured [`Self::seed`]; the caller passes the one its hook has.
    pub fn hint_chunk(&self, seed: i32, chunk_x: i32, chunk_z: i32) {
        self.terrain.hint(seed, chunk_x, chunk_z);
    }

    /// The terrain stage's first step: Reimagined's `buildTerrain` hands the chunk to
    /// `terrainCache().combineAsync(EXECUTOR, seed, chunk, ..)` with the captured [`Self::seed`] (6dd607eb's
    /// `fillFromNoise` passes `Seeds.get(state)` instead), which waits for the chunk's cached `TerrainData` at the
    /// chunk's own position and runs `fillFromNoise`'s `ChunkUtil.fillChunk` + `primeHeightmaps` with `getSeaLevel()`
    /// ([`fill_chunk`]). Returns that cache entry's data, for the later steps of the stage.
    ///
    /// Java runs the fill as a continuation on its executor; the port waits for the cache task on the calling thread
    /// and fills there (same data, same blocks). Like Java it neither hints nor drops: the entry stays cached until
    /// [`Self::drop_chunk`] (`applyBiomeDecoration`). The fill writes inside `chunk`'s own build range
    /// (`min_y()`..`min_y() + height()`), clipping terrain outside it ([`fill_chunk`]); TerraForged's levels are not
    /// rescaled to it. Not ported here: the rest of Java's `fillFromNoise` (`buildStructureTerrain`, Deep Caves,
    /// Strata) and the stage's surface and carver steps.
    ///
    /// `Err` where the chunk refuses a write ([`fill_chunk`]). Panics where Java's future completes exceptionally: a
    /// chunk whose region build failed (the cache task panicked; see [`TerrainCache::get_now`]). A failure after the
    /// region is read leaves the data unfilled and the chunk is filled from it, as in Java
    /// ([`ErodedNoiseGenerator::generate`]).
    pub fn fill_from_noise<B: TerrainBlockStates>(
        &self,
        chunk: &mut TerrainChunk<'_, B>,
        stone: BlockStateId,
        water: BlockStateId,
    ) -> Result<TerrainDataHandle<ErodedNoiseGenerator>, TerrainError> {
        let position = chunk.position();
        let terrain = self.chunk_data(self.seed(), position.x, position.z);
        fill_chunk(self.sea_level(), chunk, &terrain, stone, water)?;
        Ok(terrain)
    }

    /// `terrainCache().drop(seed, pos)`, as `applyBiomeDecoration` calls it with `Seeds.get(region.getSeed())`.
    pub fn drop_chunk(&self, seed: i32, chunk_x: i32, chunk_z: i32) {
        self.terrain.remove(seed, chunk_x, chunk_z);
    }

    /// `getBaseHeight(x, z, type, ..)` at block `(x, z)` with the captured seed: the un-eroded sample's base (water)
    /// level and the ERODED column height (`getErodedHeightNoise`, so structures stand on the eroded ground), each
    /// floored to a block; the surface types return the higher of the two plus one, the ocean-floor types the height
    /// plus one (Java `int` math). Samples the base first, as Java does. `Err` where the noise has no seed-first form
    /// (a custom module; see [`ErodedNoiseGenerator::noise_sample`]): the error is passed on rather than replaced by
    /// a height, and the caller decides (Java has no such failure).
    pub fn base_height(
        &self,
        x: i32,
        z: i32,
        heightmap: TerraHeightmap,
    ) -> Result<i32, &'static str> {
        let seed = self.seed();
        let base_noise = self.terrain.noise_sample(seed, x, z)?.base_noise;
        let eroded_height = self.terrain.eroded_height_noise(seed, x, z)?;
        Ok(base_height_from_noise(
            self.levels,
            base_noise,
            eroded_height,
            heightmap,
        ))
    }

    /// `getBaseColumn(x, z, ..)` at block `(x, z)` with the captured seed, from the UN-eroded sample (base level and
    /// raw height, as Java; only `getBaseHeight` reads the eroded height). `Err` as [`Self::base_height`]. Panics
    /// where Java throws: a negative surface (`NegativeArraySizeException`) or height (`IllegalArgumentException`
    /// from `Arrays.fill`). Java allocates a surface-long array, so a surface near 2^31 fails with
    /// `OutOfMemoryError` there; the port allocates it.
    pub fn base_column(&self, x: i32, z: i32) -> Result<BaseColumn, &'static str> {
        let sample = self.terrain.noise_sample(self.seed(), x, z)?;
        Ok(base_column_from_noise(
            self.levels,
            sample.base_noise,
            sample.height_noise,
        ))
    }
}

fn base_height_from_noise(
    levels: TerrainLevels,
    base_noise: f32,
    eroded_height_noise: f32,
    heightmap: TerraHeightmap,
) -> i32 {
    let base = levels.height(levels.scaled_base_level(base_noise));
    let height = levels.height(levels.scaled_height(eroded_height_noise));
    if heightmap.includes_water() {
        base.max(height).wrapping_add(1)
    } else {
        height.wrapping_add(1)
    }
}

fn base_column_from_noise(levels: TerrainLevels, base_noise: f32, height_noise: f32) -> BaseColumn {
    let base = levels.height(levels.scaled_base_level(base_noise));
    let height = levels.height(levels.scaled_height(height_noise));
    let surface = base.max(height);
    let length = usize::try_from(surface)
        .expect("Generator.getBaseColumn: negative surface (Java: NegativeArraySizeException)");
    let stone = usize::try_from(height)
        .expect("Generator.getBaseColumn: negative height (Java: IllegalArgumentException)");
    let mut states = vec![ColumnBlock::Stone; length];
    states[stone..].fill(ColumnBlock::Water);
    BaseColumn {
        min_y: height,
        states,
    }
}

#[cfg(test)]
mod tests {
    use super::super::{
        terraforged::Module,
        terraforged_chunk_fill::tests::{AIR, Properties, STONE, WATER, registry},
        terraforged_erosion::{ErosionFilterSettings, HeightmapSmoothingSettings},
        terraforged_noise_generator::tests::{generator, generator_with},
        terraforged_terrain_blender::TerrainNoise,
    };
    use super::*;
    use horizon_core::position::ChunkPos;
    use std::{
        panic::{AssertUnwindSafe, catch_unwind},
        sync::Arc,
        thread,
    };

    // Minecraft calls the generator from every chunk worker: the facade must be shareable.
    const _: () = {
        const fn send_sync<T: Send + Sync>() {}
        send_sync::<TerraForgedGenerator>();
    };

    /// `GeneratorQueryOracle.levels` (= the noise generator tests' level variants).
    fn levels(variant: usize) -> TerrainLevels {
        match variant {
            0 => TerrainLevels::new(false, 1.0, -64, 480, 128, 62, 22),
            1 => TerrainLevels::new(false, 1.0, -64, 256, 128, 62, 22),
            _ => TerrainLevels::new(true, 2.5, 0, 480, 200, 63, 30),
        }
    }

    fn facade(
        variant: usize,
        set: usize,
        droplets: i32,
        lifetime: i32,
        smoothing: i32,
    ) -> TerraForgedGenerator {
        TerraForgedGenerator::new(
            levels(variant),
            ErodedNoiseGenerator::new(
                generator(variant, set),
                ErosionFilterSettings {
                    droplets_per_chunk: droplets,
                    droplet_lifetime: lifetime,
                    ..ErosionFilterSettings::default()
                },
                HeightmapSmoothingSettings {
                    iterations: smoothing,
                    ..HeightmapSmoothingSettings::default()
                },
            ),
        )
    }

    fn h(value: f32) -> String {
        format!(
            "{:08x}",
            if value.is_nan() {
                0x7fc0_0000
            } else {
                value.to_bits()
            }
        )
    }

    /// The oracle's `<column>`: the column, the panic where Java throws, or `skip` past 2^20 blocks.
    fn column(levels: TerrainLevels, base_noise: f32, height_noise: f32) -> String {
        let base = levels.height(levels.scaled_base_level(base_noise));
        let height = levels.height(levels.scaled_height(height_noise));
        if base.max(height) > 1 << 20 {
            return "skip".into();
        }
        match catch_unwind(|| base_column_from_noise(levels, base_noise, height_noise)) {
            Ok(c) => {
                let stone = c
                    .states
                    .iter()
                    .filter(|s| **s == ColumnBlock::Stone)
                    .count();
                format!(
                    "{} {} {stone} {}",
                    c.min_y,
                    c.states.len(),
                    c.states.len() - stone
                )
            }
            Err(_) if base.max(height) < 0 => "throw NegativeArraySizeException".into(),
            Err(_) => "throw IllegalArgumentException".into(),
        }
    }

    const NOISE: [f32; 18] = [
        0.0,
        -0.0,
        0.25,
        0.4,
        0.1,
        -0.5,
        1.0,
        1.5,
        -1.0,
        0.999999,
        1e-7,
        -1e-7,
        f32::from_bits(1),
        3e9,
        -3e9,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NAN,
    ];

    /// `oracles/generator_facade/out.txt` (`run.sh`; interpreted and JIT-compiled alike): the levels the queries
    /// report, `Seeds.get`, and `FNV-1a` digests of the `m` lines (Reimagined's `getBaseHeight`/`getBaseColumn` math
    /// over Reimagined's own `TerrainLevels`, 18 x 18 base/height noises per level variant: signed zeros,
    /// subnormals, huge values, infinities, NaN; negative heights throw).
    const JAVA_MATH: &str = "\
L 0 -64 62 480
L 1 -64 62 256
L 2 0 63 480
S 0 0
S 1 1
S -1 -1
S 81985529216486895 -1985229329
S 1311768467177459183 -1985229329
S -9223372036854775808 0
S 9223372036854775807 -1
S 2147483648 -2147483648
S -2147483649 2147483647
m 0 324 b54f9ac7cae14345
m 1 324 31a29341ed33a700
m 2 324 650bb712eb81e13c
";

    fn fnv(text: &str) -> u64 {
        text.bytes().fold(0xcbf2_9ce4_8422_2325, |hash: u64, b| {
            (hash ^ u64::from(b)).wrapping_mul(0x100_0000_01b3)
        })
    }

    #[test]
    fn query_math_matches_reimagined_java() {
        let mut out = Vec::new();
        for variant in 0..3 {
            let l = levels(variant);
            let g = TerraForgedGenerator::new(l, facade_noise(variant));
            out.push(format!(
                "L {variant} {} {} {}",
                g.min_y(),
                g.sea_level(),
                g.gen_depth()
            ));
        }
        for seed in [
            0,
            1,
            -1,
            0x0123_4567_89ab_cdef,
            0x1234_5678_89ab_cdef,
            i64::MIN,
            i64::MAX,
            0x8000_0000,
            -0x8000_0001,
        ] {
            out.push(format!("S {seed} {}", seed_of(seed)));
        }
        for variant in 0..3 {
            let l = levels(variant);
            let mut lines = String::new();
            for b in NOISE {
                for n in NOISE {
                    let surface = base_height_from_noise(l, b, n, TerraHeightmap::WorldSurface);
                    let ocean = base_height_from_noise(l, b, n, TerraHeightmap::OceanFloor);
                    for kind in [
                        TerraHeightmap::WorldSurfaceWorldgen,
                        TerraHeightmap::MotionBlocking,
                        TerraHeightmap::MotionBlockingNoLeaves,
                    ] {
                        assert_eq!(base_height_from_noise(l, b, n, kind), surface);
                    }
                    assert_eq!(
                        base_height_from_noise(l, b, n, TerraHeightmap::OceanFloorWorldgen),
                        ocean
                    );
                    lines.push_str(&format!(
                        "m {variant} {} {} {surface} {ocean} {}\n",
                        h(b),
                        h(n),
                        column(l, b, n)
                    ));
                }
            }
            out.push(format!(
                "m {variant} {} {:016x}",
                lines.lines().count(),
                fnv(&lines)
            ));
        }
        assert_eq!(out, JAVA_MATH.lines().collect::<Vec<_>>());
    }

    /// A cheap generator for tests that never sample.
    fn facade_noise(variant: usize) -> ErodedNoiseGenerator {
        ErodedNoiseGenerator::new(
            generator(variant, 0),
            ErosionFilterSettings::default(),
            HeightmapSmoothingSettings::default(),
        )
    }

    /// `GeneratorQueryOracle.CASES`/`POS`: levels, terrain set, seed, droplets, lifetime, smoothing; block positions
    /// (negative, region-crossing, `int` limits, the world border).
    const CASES: [(usize, usize, i32, i32, i32, i32, &[(i32, i32)]); 4] = [
        (
            0,
            1,
            0,
            350,
            25,
            0,
            &[(0, 0), (5, 77), (-1, -1), (-123, -51)],
        ),
        (
            2,
            1,
            42,
            350,
            25,
            1,
            &[
                (i32::MAX, i32::MIN),
                (i32::MAX - 9, i32::MIN + 3),
                (i32::MIN, i32::MAX),
            ],
        ),
        (
            0,
            0,
            i32::MIN,
            100,
            12,
            0,
            &[
                (1000, -1000),
                (-30_000_000, 29_999_999),
                (-29_999_990, 29_999_950),
            ],
        ),
        (
            1,
            1,
            -17,
            350,
            25,
            0,
            &[(300, 300), (-777, 1234), (-770, 1240)],
        ),
    ];
    const JAVA_QUERIES: &str = "\
q 0 0 0 3df5fc22 3e339f1c 3e7e455f 120 120 84 84 84 0
q 0 5 77 3e0dc23c 3f2c6fd0 3f29702c 318 318 323 323 323 0
q 0 -1 -1 3df5fc22 3e489a6e 3e7e7eab 120 120 94 94 94 0
q 0 -123 -51 3dc97019 3f57f5c3 3f54c608 399 399 404 404 404 0
q 1 2147483647 -2147483648 00000000 3d9c28f6 3e616516 106 106 36 62 36 26
q 1 2147483638 -2147483645 00000000 3d9c28f6 3d96ded8 63 36 36 62 36 26
q 1 -2147483648 2147483647 00000000 3d9c28f6 3e7979da 117 117 36 62 36 26
q 2 1000 -1000 3ece3552 3e612d81 3e612d81 114 106 105 113 105 8
q 2 -30000000 29999999 3ee64c34 3e9ab620 3e9ab052 146 146 145 145 145 0
q 2 -29999990 29999950 3ee50bda 3e813107 3e813107 122 122 121 121 121 0
q 3 300 300 3e9f02ef 3f0ee7e3 3f0cebc6 141 141 142 142 142 0
q 3 -777 1234 3eae8d03 3f0d3be1 3f0be924 140 140 141 141 141 0
q 3 -770 1240 3eafbae6 3f060470 3f0530fb 134 134 134 134 134 0
";

    /// End to end against Reimagined's Java (`q` lines): the facade's `base_height`/`base_column` through its
    /// terrain cache, with the seed captured from a level seed whose low 32 bits are the case's seed.
    #[test]
    fn queries_match_reimagined_java() {
        let mut out = Vec::new();
        for (case, &(variant, set, seed, droplets, lifetime, smoothing, positions)) in
            CASES.iter().enumerate()
        {
            let g = facade(variant, set, droplets, lifetime, smoothing);
            g.capture_seed((0x5eed_i64 << 32) | i64::from(seed as u32));
            assert_eq!(g.seed(), seed);
            for &(x, z) in positions {
                let sample = g.terrain.noise_sample(seed, x, z).unwrap();
                let eroded = g.terrain.eroded_height_noise(seed, x, z).unwrap();
                let surface = g.base_height(x, z, TerraHeightmap::WorldSurface).unwrap();
                let ocean = g
                    .base_height(x, z, TerraHeightmap::OceanFloorWorldgen)
                    .unwrap();
                let col = g.base_column(x, z).unwrap();
                assert_eq!(
                    col,
                    base_column_from_noise(g.levels(), sample.base_noise, sample.height_noise)
                );
                out.push(format!(
                    "q {case} {x} {z} {} {} {} {surface} {ocean} {}",
                    h(sample.base_noise),
                    h(sample.height_noise),
                    h(eroded),
                    column(g.levels(), sample.base_noise, sample.height_noise)
                ));
            }
        }
        assert_eq!(out, JAVA_QUERIES.lines().collect::<Vec<_>>());
    }

    /// The seed is captured from the level seed; chunk queries go to one shared cache entry per seed and chunk,
    /// from any thread; `drop_chunk` removes it.
    #[test]
    fn facade_captures_seed_and_routes_cache_queries() {
        let g = Arc::new(facade(0, 0, 0, 25, 0));
        assert_eq!((g.seed(), g.level_seed()), (0, 0));
        let level_seed = 0x1234_5678_89ab_cdef_u64 as i64;
        g.capture_seed(level_seed);
        assert_eq!(g.seed(), 0x89ab_cdef_u32 as i32);
        assert_eq!(g.level_seed(), level_seed);
        let seed = g.seed();
        let task = g.chunk_data_async(seed, -2, 3);
        g.hint_chunk(seed, -2, 3);
        let first = task.join();
        let others: Vec<_> = (0..4)
            .map(|_| {
                let g = Arc::clone(&g);
                thread::spawn(move || g.chunk_data(seed, -2, 3))
            })
            .collect();
        for other in others {
            assert!(first.ptr_eq(&other.join().unwrap()));
        }
        assert!(!first.ptr_eq(&g.chunk_data(seed.wrapping_add(1), -2, 3)));
        g.drop_chunk(seed, -2, 3);
        assert!(!first.ptr_eq(&g.chunk_data(seed, -2, 3)));
    }

    /// A custom module has no seed-first form: both queries return the noise's `Err`, never a made-up height.
    #[test]
    fn failed_samples_are_errors() {
        let g = TerraForgedGenerator::new(
            levels(0),
            ErodedNoiseGenerator::new(
                generator_with(0, |context| {
                    vec![Arc::new(TerrainNoise::new(
                        context.terrain.get_by_name("hills").unwrap(),
                        1.0,
                        Module::custom(0.0, 1.0, |_, _| 0.25),
                    ))]
                }),
                ErosionFilterSettings::default(),
                HeightmapSmoothingSettings::default(),
            ),
        );
        for kind in [TerraHeightmap::WorldSurface, TerraHeightmap::OceanFloor] {
            assert!(g.base_height(3, -4, kind).is_err());
        }
        assert!(g.base_column(3, -4).is_err());
    }

    /// Negative heights panic where Java's `getBaseColumn` throws.
    #[test]
    fn negative_columns_panic() {
        let l = levels(0);
        assert!(catch_unwind(AssertUnwindSafe(|| base_column_from_noise(l, -1.0, -1.0))).is_err());
        assert!(catch_unwind(AssertUnwindSafe(|| base_column_from_noise(l, 0.25, -0.5))).is_err());
    }

    /// The stage fills the chunk at its own position from the cache entry of the captured seed (`Seeds.get`, not the
    /// raw level seed), leaves that entry cached and returns it; `drop_chunk` removes it.
    #[test]
    fn fill_from_noise_uses_chunk_position_and_keeps_terrain_for_later_stages() {
        let g = facade(0, 0, 0, 25, 0);
        g.capture_seed((5_i64 << 32) | 7);
        let blocks = registry();
        let mut chunk =
            TerrainChunk::new(ChunkPos::new(-2, 3), -64, 544, &blocks, Properties).unwrap();
        let terrain = g.fill_from_noise(&mut chunk, STONE, WATER).unwrap();
        assert!(terrain.ptr_eq(&g.chunk_data(7, -2, 3)));
        assert!(!terrain.ptr_eq(&g.chunk_data(7, 3, -2)));
        for z in 0..16 {
            for x in 0..16 {
                let height = terrain.height_at(x, z);
                let water = TerrainLevels::water_level(x, z, g.sea_level(), &terrain);
                let top = height.max(water);
                assert_eq!(chunk.block(x, height, z), STONE);
                assert_eq!(
                    chunk.block(x, top, z),
                    if top == height { STONE } else { WATER }
                );
                assert_eq!(chunk.block(x, top + 1, z), AIR);
            }
        }
        g.drop_chunk(7, -2, 3);
        assert!(!terrain.ptr_eq(&g.chunk_data(7, -2, 3)));
    }
}
