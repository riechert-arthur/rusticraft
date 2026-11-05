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
// `getBaseHeight`, `getBaseColumn`, `buildTerrain`'s hand-off to `fillFromNoise`, and `createBiomes`), `Seeds.java`
// and `worldgen/biome/Source.java` (`withSeed`, `getSurfaceBiome`, `getLayerBiome`, `compute`). MIT License,
// Copyright (c) 2020 TerraForged, Copyright (c) 2026 Romoslayer; see `LICENSE-TERRAFORGED-REIMAGINED`.

use super::{
    super::biome_chunk::{BiomeChunk, BiomeFillError},
    super::terrain::{TerrainBlockStates, TerrainChunk, TerrainError},
    terraforged_biome_chunk_fill::fill_noise_biomes,
    terraforged_biome_map::BiomePools,
    terraforged_biome_sampler::{BiomeOverride, WetlandSettings},
    terraforged_cave_biome_sampler::CaveBiomeSampler,
    terraforged_chunk_fill::fill_chunk,
    terraforged_current_surface_biomes::CurrentSurfaceBiomeSampler,
    terraforged_eroded_noise_generator::ErodedNoiseGenerator,
    terraforged_noise_generator::NoiseGenerator,
    terraforged_terrain_cache::{TerrainCache, TerrainDataHandle, TerrainTask},
    terraforged_terrain_levels::TerrainLevels,
};
use crate::registry::BlockStateId;
use horizon_core::position::ChunkPos;
use std::sync::Arc;
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

    /// The un-eroded noise generator under the terrain cache (Reimagined's `ErodedNoiseGenerator` delegates
    /// `getHeightNoise`, the levels and the continent to it), which the biome samplers read.
    pub fn noise_generator(&self) -> &NoiseGenerator {
        self.terrain.noise_generator().generator()
    }

    /// `new BiomeSampler(noiseGenerator, biomeMapManager)` over this generator's noise, as `Source`'s memoized
    /// `Parts` builds it: see [`CurrentSurfaceBiomeSampler::new`]. Building it draws the climate noise, which is not
    /// cheap: build ONE per world and share it across chunk workers (it is `Send + Sync`), never one per chunk.
    #[allow(clippy::too_many_arguments)]
    pub fn surface_biome_sampler<'a, T>(
        &'a self,
        wet: &'a BiomePools<T>,
        dry: &'a BiomePools<T>,
        highland: &'a BiomePools<T>,
        plains: &'a Arc<T>,
        wetlands: WetlandSettings,
        mountain_usage: f32,
    ) -> CurrentSurfaceBiomeSampler<'a, T> {
        CurrentSurfaceBiomeSampler::new(
            self.noise_generator(),
            wet,
            dry,
            highland,
            plains,
            wetlands,
            mountain_usage,
        )
    }

    /// Reimagined's `createBiomes`: `terrainCache().hint(seed, chunk.getPos())` with the captured [`Self::seed`],
    /// then `ChunkUtil.fillNoiseBiomes(chunk, biomeSource, ..)` ([`fill_noise_biomes`]) over `Source`'s two
    /// layers at the same seed (`Source.withSeed` keeps `(int) levelSeed`, which is `Seeds.get`). Per quart column
    /// (quart `x`, `z`): `getSurfaceBiome` = `compute`'s `sampleBiome(seed, x << 2, z << 2)` on `surface` (block
    /// coordinates, Java `int` wrap), `getLayerBiome` = `cave.get_layer_biome(seed, x << 2, z << 2)`. The hint comes
    /// first, as Java hints before it schedules the fill; Java fills on its executor, the port on the calling
    /// thread. `surface` must come from [`Self::surface_biome_sampler`] (built once per world; panics if it reads
    /// another generator's noise); `cave` is Reimagined's `CaveBiomeSampler(800, manager)`, also built once.
    /// `base_temperature` and `override_biome` are as in [`CurrentSurfaceBiomeSampler::sample_biome`].
    ///
    /// `Err` with the surface sampler's first error (a custom module with no seed-first form, where Java's future
    /// fails) before any section is written; the hint has happened by then, as in Java. Not ported: `Source`'s
    /// per-column `LossyCache` of surface biomes (a cache of the same values: `createBiomes` asks each quart column
    /// once, so only repeated queries would hit it) and its debug `reportCaveLayer` survey.
    pub fn create_biomes<T: Clone>(
        &self,
        chunk: &mut BiomeChunk<T>,
        position: ChunkPos,
        surface: &CurrentSurfaceBiomeSampler<'_, T>,
        cave: &CaveBiomeSampler<T>,
        base_temperature: impl Fn(&T) -> f32,
        override_biome: impl Fn(BiomeOverride) -> Arc<T>,
    ) -> Result<(), BiomeFillError<&'static str>> {
        assert!(
            std::ptr::eq(surface.noise(), self.noise_generator()),
            "create_biomes: the surface sampler was not built over this generator"
        );
        let seed = self.seed();
        self.terrain.hint(seed, position.x, position.z);
        fill_biomes(
            seed,
            chunk,
            position,
            surface,
            cave,
            base_temperature,
            override_biome,
        )
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

/// [`TerraForgedGenerator::create_biomes`]'s fill at `seed`, without the hint.
fn fill_biomes<T: Clone>(
    seed: i32,
    chunk: &mut BiomeChunk<T>,
    position: ChunkPos,
    surface: &CurrentSurfaceBiomeSampler<'_, T>,
    cave: &CaveBiomeSampler<T>,
    base_temperature: impl Fn(&T) -> f32,
    override_biome: impl Fn(BiomeOverride) -> Arc<T>,
) -> Result<(), BiomeFillError<&'static str>> {
    fill_noise_biomes(
        chunk,
        position,
        |x, z| {
            surface
                .sample_biome(
                    seed,
                    x.wrapping_shl(2),
                    z.wrapping_shl(2),
                    &base_temperature,
                    &override_biome,
                )
                .map(|biome| T::clone(&biome))
        },
        |x, z| {
            Ok(cave
                .get_layer_biome(seed, x.wrapping_shl(2), z.wrapping_shl(2))
                .map(|biome| T::clone(biome)))
        },
    )
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
        terraforged_chunk_fill::tests::{AIR, Properties, STONE, WATER, digest, registry},
        terraforged_current_biome_sampler::tests::generator as climate_generator,
        terraforged_current_surface_biomes::tests::{CHANCE, NAMES, TEMPS, USAGE, WETLANDS, pools},
        terraforged_engine::BiomeType,
        terraforged_erosion::{ErosionFilterSettings, HeightmapSmoothingSettings},
        terraforged_noise_generator::tests::{generator, generator_with},
        terraforged_terrain_blend_util::WeightMap,
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

    /// `TerrainStageOracle` cases (`oracles/generator_facade/run-stage.sh`): levels, terrain set, captured seed,
    /// droplets, lifetime, smoothing; chunks.
    const STAGE_CASES: [(usize, usize, i32, i32, i32, i32, &[(i32, i32)]); 4] = [
        (0, 1, 0, 350, 25, 0, &[(0, 0), (-1, -1), (5, 3)]),
        (2, 1, 42, 350, 25, 1, &[(0, 0), (-3, 2)]),
        (1, 1, -17, 350, 25, 0, &[(18, -48)]),
        (0, 0, i32::MIN, 100, 12, 0, &[(-1_875_000, 1_874_999)]),
    ];
    /// Build ranges (min Y, height): vanilla 26.3, 544 blocks from -64, Reimagined's widened overworld, 0..256 (one
    /// chunk's terrain rises above its top), 128..192 (all terrain is below its bottom).
    const STAGE_RANGES: [(i32, i32); 5] =
        [(-64, 384), (-64, 544), (-64, 1024), (0, 256), (128, 64)];

    /// `oracles/generator_facade/stage.txt` (interpreted and JIT-compiled alike): Reimagined's `buildTerrain` hand-off
    /// end to end -- its real `TerrainCache.combineAsync` over its `TerrainGenerator`/`TerrainData` and the eroded
    /// generator, `fillChunk` + `primeHeightmaps` copied onto a stub chunk. `t` lines: the data's section bounds; `f`
    /// lines: digests of every block and both heightmaps per build range (Java filled in a -4096..4096 chunk and read
    /// through the range; `same`, or the exception Java throws in a chunk of exactly that range). The oracle's `h`
    /// lines (the -64..320 heightmaps spelled out, for diffs) are covered by the digests and not embedded.
    const JAVA_STAGE: &str = "\
t 0 0 0 119 210 77
f 0 0 0 -64 384 f58f1bfde49a187b same
f 0 0 0 -64 544 39de228e805d987b same
f 0 0 0 -64 1024 4edfb9d4d3a8187b same
f 0 0 0 0 256 2de995f6f5b1d87b same
f 0 0 0 128 64 d7ef3c13c555ced8 throws ArrayIndexOutOfBoundsException
t 0 -1 -1 119 214 77
f 0 -1 -1 -64 384 caa11efaa5e6c0bc same
f 0 -1 -1 -64 544 39a16fd642bcc0bc same
f 0 -1 -1 -64 1024 bd6d6d73193ec0bc same
f 0 -1 -1 0 256 f9e29f6c435d80bc same
f 0 -1 -1 128 64 92849516840a2798 throws ArrayIndexOutOfBoundsException
t 0 5 3 193 288 81
f 0 5 3 -64 384 abf3d479618ddf2d same
f 0 5 3 -64 544 bd8cbd2d1df85f2d same
f 0 5 3 -64 1024 491262e5d337df2d same
f 0 5 3 0 256 8dc8a4422d0d48e2 throws ArrayIndexOutOfBoundsException
f 0 5 3 128 64 620e49ea1d1c0325 throws ArrayIndexOutOfBoundsException
t 1 0 0 104 111 109
f 1 0 0 -64 384 5f05181bdc1e6225 same
f 1 0 0 -64 544 de4417169afae225 same
f 1 0 0 -64 1024 f2bf37d257906225 same
f 1 0 0 0 256 701347425f2c2225 same
f 1 0 0 128 64 5ed8a67d98b3c325 throws ArrayIndexOutOfBoundsException
t 1 -3 2 112 125 108
f 1 -3 2 -64 384 e91ad63cb87323b9 same
f 1 -3 2 -64 544 9355fb38b901a3b9 same
f 1 -3 2 -64 1024 7810aae63aad23b9 same
f 1 -3 2 0 256 368c741f546ce3b9 same
f 1 -3 2 128 64 5ed8a67d98b3c325 throws ArrayIndexOutOfBoundsException
t 2 18 -48 90 92 89
f 2 18 -48 -64 384 b94f0ac577864e75 same
f 2 18 -48 -64 544 9da8c821148ace75 same
f 2 18 -48 -64 1024 e53d5e576b984e75 same
f 2 18 -48 0 256 b26e6d146e840e75 same
f 2 18 -48 128 64 5ed8a67d98b3c325 throws ArrayIndexOutOfBoundsException
t 3 -1875000 1874999 130 145 120
f 3 -1875000 1874999 -64 384 fdd51dfaf54e6eb8 same
f 3 -1875000 1874999 -64 544 328463fba5aa6eb8 same
f 3 -1875000 1874999 -64 1024 80391461b6be6eb8 same
f 3 -1875000 1874999 0 256 172da0aebe292eb8 same
f 3 -1875000 1874999 128 64 3bed1c90e336aeb8 same
";

    /// End to end against Reimagined's Java: whole filled chunks and their heightmaps, bit for bit, in every build
    /// range (clipped where Java throws, as `fill_chunk` documents).
    #[test]
    fn terrain_stage_matches_reimagined_java() {
        let blocks = registry();
        let mut java = JAVA_STAGE.lines();
        for (case, &(variant, set, seed, droplets, lifetime, smoothing, chunks)) in
            STAGE_CASES.iter().enumerate()
        {
            let g = facade(variant, set, droplets, lifetime, smoothing);
            g.capture_seed((-0x5eed_i64 << 32) | i64::from(seed as u32));
            assert_eq!(g.seed(), seed);
            for &(cx, cz) in chunks {
                let terrain = g.chunk_data(seed, cx, cz);
                let line = format!(
                    "t {case} {cx} {cz} {} {} {}",
                    terrain.min(),
                    terrain.max(),
                    terrain.max_base()
                );
                assert_eq!(java.next(), Some(line.as_str()));
                for (min_y, height) in STAGE_RANGES {
                    let mut chunk = TerrainChunk::new(
                        ChunkPos::new(cx, cz),
                        min_y,
                        height,
                        &blocks,
                        Properties,
                    )
                    .unwrap();
                    let filled = g.fill_from_noise(&mut chunk, STONE, WATER).unwrap();
                    assert!(filled.ptr_eq(&terrain));
                    let expected = java.next().unwrap();
                    let prefix = format!(
                        "f {case} {cx} {cz} {min_y} {height} {:016x} ",
                        digest(&chunk)
                    );
                    assert!(expected.starts_with(&prefix), "{expected} vs {prefix}");
                }
                g.drop_chunk(seed, cx, cz);
            }
        }
        assert_eq!(java.next(), None);
    }

    /// The pools of [`create_biomes_hints_and_fills_both_layers`]: every climate type gets one biome.
    fn one(biome: &Arc<&'static str>) -> BiomePools<&'static str> {
        let mut pools = BiomePools::default();
        for climate in BiomeType::VALUES {
            pools.insert(
                climate,
                Arc::new(WeightMap::new(vec![Arc::clone(biome)], &[1.0])),
            );
        }
        pools
    }

    fn named(replacement: BiomeOverride) -> Arc<&'static str> {
        Arc::new(replacement.key().unwrap())
    }

    /// `createBiomes` hints the chunk with the captured (narrowed) seed, then fills it at that seed from both layers;
    /// a sampler built over another generator is refused; a failing surface sample is an `Err` that writes nothing.
    #[test]
    fn create_biomes_hints_and_fills_both_layers() {
        let g = facade(0, 0, 0, 25, 0);
        g.capture_seed((5_i64 << 32) | 7);
        let (land, cave_biome) = (Arc::new("land"), Arc::new("minecraft:lush_caves"));
        let pools = one(&land);
        let plains = Arc::new("plains");
        let surface = g.surface_biome_sampler(&pools, &pools, &pools, &plains, WETLANDS, 1.0);
        let cave = CaveBiomeSampler::new(800, Vec::new(), vec![Arc::clone(&cave_biome)]);
        // A chunk whose columns all pass the cave mask at seed 7 and one where none does (found by search).
        let mask = |position: ChunkPos| {
            let cave = &cave;
            (0..16).filter(move |i| {
                let x = (position.x.wrapping_shl(4) >> 2)
                    .wrapping_add(i & 3)
                    .wrapping_shl(2);
                let z = (position.z.wrapping_shl(4) >> 2)
                    .wrapping_add(i >> 2)
                    .wrapping_shl(2);
                cave.get_layer_biome(7, x, z).is_some()
            })
        };
        let mut caves = (0..4096)
            .map(|i| ChunkPos::new(i % 64 * 7 - 200, i / 64 * 7 - 200))
            .filter(|p| mask(*p).count() == 16);
        let deep = caves.next().unwrap();
        let shallow = (0..4096)
            .map(|i| ChunkPos::new(i % 64 * 7 - 200, i / 64 * 7 - 200))
            .find(|p| mask(*p).count() == 0)
            .unwrap();
        for position in [deep, shallow] {
            let mut chunk = BiomeChunk::new(position, -64, 384, "").unwrap();
            assert!(g.terrain.is_empty());
            g.create_biomes(&mut chunk, position, &surface, &cave, |_| 0.8, named)
                .unwrap();
            // Hinted with `Seeds.get` of the level seed (7), not the raw level seed.
            assert_eq!(g.terrain.len(), 1);
            g.drop_chunk(7, position.x, position.z);
            assert!(g.terrain.is_empty());

            let mut direct = BiomeChunk::new(position, -64, 384, "").unwrap();
            fill_biomes(7, &mut direct, position, &surface, &cave, |_| 0.8, named).unwrap();
            let cells = |chunk: &BiomeChunk<&'static str>| {
                (0..24)
                    .flat_map(|i| chunk.section(i).unwrap().to_vec())
                    .collect::<Vec<_>>()
            };
            assert_eq!(cells(&chunk), cells(&direct));
            for quart in 0..16 {
                let (x, z) = (quart & 3, quart >> 2);
                let column = [
                    position.x.wrapping_shl(2) + x,
                    0,
                    position.z.wrapping_shl(2) + z,
                ];
                let expected = surface
                    .sample_biome(7, column[0] << 2, column[2] << 2, |_| 0.8, named)
                    .unwrap();
                // Surface layer from y 0 up, cave layer in sections wholly below y 0 where the mask passes.
                for y in [0, 16, 79] {
                    assert_eq!(*chunk.noise_biome([column[0], y, column[2]]), *expected);
                }
                let below = *chunk.noise_biome([column[0], -1, column[2]]);
                if position == deep {
                    assert_eq!(below, *cave_biome);
                } else {
                    assert_eq!(below, *expected);
                }
            }
        }

        let other = facade(0, 0, 0, 25, 0);
        let foreign = other.surface_biome_sampler(&pools, &pools, &pools, &plains, WETLANDS, 1.0);
        let mut chunk = BiomeChunk::new(deep, -64, 384, "").unwrap();
        assert!(
            catch_unwind(AssertUnwindSafe(|| {
                g.create_biomes(&mut chunk, deep, &foreign, &cave, |_| 0.8, named)
            }))
            .is_err()
        );

        let failing = TerraForgedGenerator::new(
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
        let surface = failing.surface_biome_sampler(&pools, &pools, &pools, &plains, WETLANDS, 1.0);
        // The chunk holding block (3, -4), where `failed_samples_are_errors`' height noise fails.
        let land = ChunkPos::new(0, -1);
        let mut chunk = BiomeChunk::new(land, -64, 384, "unset").unwrap();
        // The fill without the hint (the hinted terrain task would fail on a worker thread, as Java's future does).
        assert!(matches!(
            fill_biomes(
                failing.seed(),
                &mut chunk,
                land,
                &surface,
                &cave,
                |_| 0.8,
                named
            ),
            Err(BiomeFillError::Resolve(_))
        ));
        assert!((0..24).all(|i| chunk.section(i).unwrap().iter().all(|b| *b == "unset")));
    }

    /// `.local/terraforged/oracles/chunk_biomes/` (`run.sh`, after `../noise_generator/run.sh`): half A records what
    /// `sampleBiome` reads at every quart column of [`CHUNKS`] from 6dd607eb's real `Sampler` and the newer source's
    /// `NoiseGenerator`; half B runs Reimagined a24d450's `ChunkUtil.fillNoiseBiomes` and `Source` members (verbatim)
    /// with its real `BiomeBuffer2D`, `BiomeSampler` and `CaveBiomeSampler(800, ..)` over that record, into a stub of
    /// 26.3's chunk sections, interpreted and with `-Xcomp`. Per levels variant, terrain set and configuration `k`
    /// (`current_surface_biomes`' pools and settings; cave registry [`CAVES`]`[k % 3]`): the chunk count and a fold of
    /// every cell's biome name of every chunk in each of [`RANGES`], over the six runtime seeds (level seeds
    /// `0x5eed << 32 | seed`); then every name's cell count.
    const JAVA_CHUNK_BIOMES: &str = "\
chunks 0 0 0 612 d4c7cad5da07f811\n\
chunks 0 0 1 612 51716f377b22a051\n\
chunks 0 0 2 612 745a7ce6694001f1\n\
chunks 0 0 3 612 52ac1e830a431a71\n\
chunks 0 1 0 612 487fc13996d41f71\n\
chunks 0 1 1 612 fd7283d7ea631e31\n\
chunks 0 1 2 612 b60dc3edb5c85331\n\
chunks 0 1 3 612 15857e069c5b05f1\n\
chunks 1 0 0 612 51b91c0c6a023bd1\n\
chunks 1 0 1 612 51716f377b22a051\n\
chunks 1 0 2 612 3733ff00527d7971\n\
chunks 1 0 3 612 34d91d227c3adc31\n\
chunks 1 1 0 612 4a51d8dbb3774ab1\n\
chunks 1 1 1 612 370e149de7f19b31\n\
chunks 1 1 2 612 30be269c281b88f1\n\
chunks 1 1 3 612 af8c7e212939cff1\n\
chunks 2 0 0 612 db3e34095f2cccf1\n\
chunks 2 0 1 612 e16b9b3746229951\n\
chunks 2 0 2 612 3ee8c325c15ca71\n\
chunks 2 0 3 612 f7f74751ff6c5fd1\n\
chunks 2 1 0 612 cdeb6f2d15c64c71\n\
chunks 2 1 1 612 7811194c655e47b1\n\
chunks 2 1 2 612 14c3fcfe068e0471\n\
chunks 2 1 3 612 be86604fd2db591\n\
names birch_forest=1188800 desert=893184 forest=800128 grove=1478048 jungle=3426688 meadow=2668736 \
minecraft:beach=1089536 minecraft:deep_cold_ocean=1749888 minecraft:deep_frozen_ocean=7199744 \
minecraft:deep_lukewarm_ocean=2437120 minecraft:deep_ocean=6500480 minecraft:dripstone_caves=874752 \
minecraft:frozen_river=466944 minecraft:lush_caves=600576 minecraft:river=6505280 plains=545280 \
snowy_plains=852736 swamp=3688736 taiga=669280 terraforged:cave=1253376 \
terraforged:cave/deep=1031424 windswept_hills=1707552\n\
";

    const CHUNKS: [(i32, i32); 17] = [
        (0, 0),
        (-1, -1),
        (-2, 3),
        (7, -5),
        (100, -100),
        (-250, 400),
        (97, -61),
        (194, -122),
        (291, -183),
        (-388, 244),
        (485, 305),
        (-582, -366),
        (1_874_999, -1_875_000),
        ((1 << 27) - 1, -(1 << 27)),
        (1 << 27, -(1 << 27) - 1),
        (i32::MAX, i32::MIN),
        (0x1234_5678, -0x0abc_def1),
    ];
    const RANGES: [(i32, i32); 6] = [
        (-64, 384),
        (0, 256),
        (-2032, 4064),
        (128, 64),
        (-64, 64),
        (-16, 32),
    ];
    const CAVES: [&[&str]; 3] = [
        &[
            "minecraft:dripstone_caves",
            "terraforged:cave",
            "terraforged:cave/deep",
            "minecraft:lush_caves",
        ],
        &["terraforged:cave"],
        &[],
    ];
    const SEEDS: [i32; 6] = [0, 42, -17, i32::MAX, i32::MIN, -8_763_214];

    /// `new CaveBiomeSampler(800, manager)` over the oracle's registry (the palette, then `caves`): GLOBAL is
    /// `terraforged:cave`, UNIQUE the registry's biomes in order whose path starts with `cave/` or ends with `_caves`.
    fn cave_sampler(caves: &[&'static str]) -> CaveBiomeSampler<&'static str> {
        let path = |name: &str| {
            name.split_once(':')
                .map_or(name, |(_, path)| path)
                .to_string()
        };
        let global = caves
            .iter()
            .filter(|name| **name == "terraforged:cave")
            .map(|name| Arc::new(*name))
            .collect();
        let special = caves
            .iter()
            .filter(|name| {
                let path = path(name);
                path.starts_with("cave/") || path.ends_with("_caves")
            })
            .map(|name| Arc::new(*name))
            .collect();
        CaveBiomeSampler::new(800, global, special)
    }

    fn chunk_biome_lines() -> Vec<String> {
        let palette: Vec<Arc<&'static str>> = NAMES.iter().map(|name| Arc::new(*name)).collect();
        let plains = Arc::new("plains");
        let temperature = |biome: &&'static str| {
            NAMES
                .iter()
                .position(|name| name == biome)
                .map_or(0.8, |i| TEMPS[i])
        };
        let configs: Vec<_> = (0..4).map(|k| pools(&palette, k)).collect();
        let caves: Vec<_> = (0..3).map(|c| cave_sampler(CAVES[c])).collect();
        let mut counts = std::collections::BTreeMap::new();
        let mut hashes = std::collections::HashMap::new();
        let mut lines = Vec::new();
        for variant in 0..3 {
            for set in 0..2 {
                let mut folds = [(0, 0xcbf2_9ce4_8422_2325_u64); 4];
                for seed in SEEDS {
                    let g = TerraForgedGenerator::new(
                        levels(variant),
                        ErodedNoiseGenerator::new(
                            climate_generator(variant, set),
                            ErosionFilterSettings::default(),
                            HeightmapSmoothingSettings::default(),
                        ),
                    );
                    g.capture_seed((0x5eed_i64 << 32) | i64::from(seed as u32));
                    assert_eq!(g.seed(), seed);
                    for (k, [wet, dry, highland]) in configs.iter().enumerate() {
                        let wetlands = WetlandSettings {
                            chance: CHANCE[k],
                            ..WETLANDS
                        };
                        let surface = g
                            .surface_biome_sampler(wet, dry, highland, &plains, wetlands, USAGE[k]);
                        let (chunks, hash) = &mut folds[k];
                        for (x, z) in CHUNKS {
                            let position = ChunkPos::new(x, z);
                            for (min_y, height) in RANGES {
                                let mut chunk =
                                    BiomeChunk::new(position, min_y, height, "").unwrap();
                                fill_biomes(
                                    g.seed(),
                                    &mut chunk,
                                    position,
                                    &surface,
                                    &caves[k % 3],
                                    temperature,
                                    |replacement| Arc::new(replacement.key().unwrap()),
                                )
                                .unwrap();
                                let mut i = 0;
                                while let Some(section) = chunk.section(i) {
                                    for name in section {
                                        let value =
                                            *hashes.entry(*name).or_insert_with(|| fnv(name));
                                        *hash = (*hash ^ value).wrapping_mul(0x100_0000_01b3);
                                        *counts.entry(*name).or_insert(0_u64) += 1;
                                    }
                                    i += 1;
                                }
                                *hash = (*hash ^ 0xff).wrapping_mul(0x100_0000_01b3);
                                *chunks += 1;
                            }
                        }
                    }
                }
                for (k, (chunks, hash)) in folds.iter().enumerate() {
                    lines.push(format!("chunks {variant} {set} {k} {chunks} {hash:x}"));
                }
            }
        }
        let mut names = String::from("names");
        for (name, count) in counts {
            names.push_str(&format!(" {name}={count}"));
        }
        lines.push(names);
        lines
    }

    #[test]
    fn chunk_biomes_match_reimagined_java() {
        let expected: Vec<&str> = JAVA_CHUNK_BIOMES.lines().collect();
        let actual = chunk_biome_lines();
        assert_eq!(actual.len(), expected.len());
        for (actual, expected) in actual.iter().zip(expected) {
            assert_eq!(actual, expected);
        }
    }
}
