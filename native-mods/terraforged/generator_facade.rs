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
// `getBaseColumn` and `buildTerrain`'s hand-off to `fillFromNoise`) and `Seeds.java`. MIT License,
// Copyright (c) 2020 TerraForged, Copyright (c) 2026 Romoslayer; see `LICENSE-TERRAFORGED-REIMAGINED`.
// `create_biomes` and `base_height` follow TerraForged 6dd607eb
// (https://github.com/TerraForged/TerraForged/tree/6dd607ebfd41a3b7274e090b1f2cf565d53d1d44, MIT License,
// Copyright (c) 2021 TerraForged): `Generator.createBiomes` (terrain-cache hint, then `ChunkUtil.fillNoiseBiomes`),
// `worldgen/biome/Source.java` (`withSeed`, `getNoiseBiome`, `compute`) and `Generator.getBaseHeight` (the raw
// sample's height; Reimagined reads the eroded height there instead); `fill_from_noise_with_structures` is
// 6dd607eb's `Generator.fillFromNoise` continuation (`fillChunk`, `primeHeightmaps`, `buildStructureTerrain`;
// Reimagined's `fillFromNoise` starts the same way).

use super::{
    super::biome_chunk::{BiomeChunk, BiomeFillError},
    super::terrain::{TerrainBlockStates, TerrainChunk, TerrainError},
    terraforged_biome_chunk_fill::fill_noise_biomes,
    terraforged_biome_map::BiomePools,
    terraforged_biome_sampler::BiomeOverride,
    terraforged_chunk_fill::fill_chunk,
    terraforged_current_surface_biomes::CurrentSurfaceBiomeSampler,
    terraforged_eroded_noise_generator::ErodedNoiseGenerator,
    terraforged_noise_generator::NoiseGenerator,
    terraforged_structure_terrain::StructureTerrain,
    terraforged_surface_height_cache,
    terraforged_terrain_cache::{TerrainCache, TerrainDataHandle, TerrainTask},
    terraforged_terrain_levels::TerrainLevels,
};
use crate::registry::BlockStateId;
use horizon_core::position::ChunkPos;
use std::collections::BTreeMap;
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

/// The queries Minecraft's `ChunkGenerator` asks Reimagined's `Generator` (6dd607eb's for [`Self::base_height`]), over
/// its `TerrainCache` of the eroded generator. Heights count from block 0 as TerraForged's levels do (`levels.max_y` is the exclusive top, 480 by
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
    /// rescaled to it. [`Self::fill_from_noise_with_structures`] adds `buildStructureTerrain`; not ported here: Deep
    /// Caves, Strata and the stage's surface and carver steps.
    ///
    /// The fill reads the entry's generated heights ([`TerrainDataHandle::write_generated`]: any structure heights an
    /// earlier run of the stage wrote are undone first, so a chunk generated again gets the same blocks).
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
        fill_chunk(
            self.sea_level(),
            chunk,
            &terrain.write_generated(),
            stone,
            water,
        )?;
        Ok(terrain)
    }

    /// TerraForged 6dd607eb's whole `fillFromNoise` continuation: [`Self::fill_from_noise`], then
    /// `ChunkUtil.buildStructureTerrain(chunk, terrainData, structureManager)` ([`StructureTerrain::modify_chunk`]),
    /// which edits the chunk's blocks and writes the edited columns' OCEAN_FLOOR_WG heights into the entry's
    /// `TerrainData`. The returned handle (and every other handle of the entry: [`Self::chunk_data`] for the same seed
    /// and chunk until [`Self::drop_chunk`]) then reads those heights, as Java's surface, carver and decoration stages
    /// read the same `TerrainData` object. The fill and the structure edits run under one write guard, so no reader
    /// sees a half-adjusted chunk; a rerun starts from the generated heights, never from the previous run's.
    ///
    /// `structures` is Java's `new StructureTerrain(chunk, structureManager)`: build it with
    /// [`StructureTerrain::from_starts`] over the generator's `references(..)` for this chunk (the beardifier's list)
    /// and `chunk.position()`. `stone`, `water` and `air` are the default states of `minecraft:stone`,
    /// `minecraft:water` and `minecraft:air` (`ChunkUtil.FILLER` and `StructureTerrain`'s `solid`/`air`).
    ///
    /// Run one of the two stages per chunk: [`Self::fill_from_noise`] on the same entry sets the generated heights
    /// back, so mixing them would leave the later stages reading whichever ran last. Concurrent runs for one entry
    /// serialize on its write guard and leave the same data. No guard of the entry may be held on the calling thread.
    ///
    /// `Err` where the chunk refuses a write, in either step, with the chunk and the entry's heights partly edited
    /// (the next run starts again from the generated heights); panics as [`Self::fill_from_noise`] does.
    pub fn fill_from_noise_with_structures<B: TerrainBlockStates>(
        &self,
        chunk: &mut TerrainChunk<'_, B>,
        stone: BlockStateId,
        water: BlockStateId,
        air: BlockStateId,
        structures: &StructureTerrain,
    ) -> Result<TerrainDataHandle<ErodedNoiseGenerator>, TerrainError> {
        let position = chunk.position();
        let terrain = self.chunk_data(self.seed(), position.x, position.z);
        {
            let mut data = terrain.write_generated();
            fill_chunk(self.sea_level(), chunk, &data, stone, water)?;
            structures.modify_chunk(chunk, &mut data, stone, air)?;
        }
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
    pub fn surface_biome_sampler<'a, T>(
        &'a self,
        pools: &'a BiomePools<T>,
        plains: &'a Arc<T>,
    ) -> CurrentSurfaceBiomeSampler<'a, T> {
        CurrentSurfaceBiomeSampler::new(self.noise_generator(), pools, plains)
    }

    /// TerraForged 6dd607eb's `createBiomes`: `terrainCache.hint(Seeds.get(state), chunk.getPos())` with the captured
    /// [`Self::seed`], then `ChunkUtil.fillNoiseBiomes(chunk, biomeSource, ..)` ([`fill_noise_biomes`]) at the same seed
    /// (`Source.withSeed` keeps `(int) levelSeed`, which is `Seeds.get`). Per quart column (quart `x`, `z`),
    /// `Source.getNoiseBiome(x, -1, z, ..)` ignores `y` and is `compute`'s `sampleBiome(seed, x << 2, z << 2)` on
    /// `surface` (block coordinates, Java `int` wrap), so every height of a column gets its surface biome: there is no
    /// cave layer (TerraForged's cave biomes come from its noise-cave carver). The hint comes first, as Java hints
    /// before it schedules the fill; Java fills on its executor, the port on the calling thread. `surface` must come
    /// from [`Self::surface_biome_sampler`] (built once per world; panics if it reads another generator's noise).
    /// `override_biome` is as in [`CurrentSurfaceBiomeSampler::sample_biome`] (6dd607eb's `sampleBiome`).
    ///
    /// `Err` with the surface sampler's first error (a column whose pool has no biome for its noise, where Java's
    /// `sampleBiome` returns null and fails later) before any section is written; the hint has happened by then. Not
    /// ported: `Source`'s per-column `LossyCache` of biomes (a cache of the same values: `createBiomes` asks each quart
    /// column once, so only repeated queries would hit it).
    pub fn create_biomes<T: Clone>(
        &self,
        chunk: &mut BiomeChunk<T>,
        position: ChunkPos,
        surface: &CurrentSurfaceBiomeSampler<'_, T>,
        override_biome: impl Fn(BiomeOverride) -> Arc<T>,
    ) -> Result<(), BiomeFillError<&'static str>> {
        assert!(
            std::ptr::eq(surface.noise(), self.noise_generator()),
            "create_biomes: the surface sampler was not built over this generator"
        );
        let seed = self.seed();
        self.terrain.hint(seed, position.x, position.z);
        fill_biomes(seed, chunk, position, surface, override_biome)
    }

    /// The biomes [`Self::create_biomes`] gives `position`, without its terrain-cache hint: for a caller that reads a
    /// chunk's biomes again (the surface stage's 3x3 region around another chunk), where Java reads the biomes that
    /// chunk's own `createBiomes` stored. Hinting there would start (or, after [`Self::drop_chunk`], restart) that
    /// chunk's terrain generation, and nothing would drop it. Same `surface` rule and errors as
    /// [`Self::create_biomes`].
    pub fn sample_chunk_biomes<T: Clone>(
        &self,
        chunk: &mut BiomeChunk<T>,
        position: ChunkPos,
        surface: &CurrentSurfaceBiomeSampler<'_, T>,
        override_biome: impl Fn(BiomeOverride) -> Arc<T>,
    ) -> Result<(), BiomeFillError<&'static str>> {
        assert!(
            std::ptr::eq(surface.noise(), self.noise_generator()),
            "sample_chunk_biomes: the surface sampler was not built over this generator"
        );
        fill_biomes(self.seed(), chunk, position, surface, override_biome)
    }

    /// TerraForged 6dd607eb's `NoiseChunkUtil.getNoiseChunk` → `initChunk` → `initSurfaceCache` for `chunk`: asks
    /// for the chunk's terrain task with the captured [`Self::seed`] (Java's `getChunkDataAsync(Seeds.get(state),
    /// pos)`, which starts the generation even when the cache turns out to be filled), then, only if `cache` is empty,
    /// waits for it and fills `cache` from the entry's current heights
    /// ([`terraforged_surface_height_cache::initialize`]). A filled cache is left as it is, as in Java.
    ///
    /// The heights are whatever the shared entry holds when this runs: after
    /// [`Self::fill_from_noise_with_structures`] they include the structure edits, as Java's surface stage reads the
    /// `TerrainData` its `fillFromNoise` edited; run it after the chunk's terrain stage, never alongside a different
    /// stage kind on the same entry. They count in TerraForged's levels and are not clipped to the chunk's build range
    /// (edited columns carry the clipped OCEAN_FLOOR_WG heights `modify_chunk` wrote), so mapping them to 26.3's
    /// world height is the registering layer's job, with the open world-height decision. No guard of the entry may
    /// be held on the calling thread while the stage that writes it runs. Panics as [`Self::chunk_data`] does.
    pub fn initialize_surface_cache(&self, chunk: ChunkPos, cache: &mut BTreeMap<i64, i32>) {
        let task = self.chunk_data_async(self.seed(), chunk.x, chunk.z);
        if !cache.is_empty() {
            return;
        }
        let terrain = task.join();
        let data = terrain.read();
        terraforged_surface_height_cache::initialize(cache, chunk, |x, z| data.height_at(x, z));
    }

    /// `terrainCache().drop(seed, pos)`, as `applyBiomeDecoration` calls it with `Seeds.get(region.getSeed())`.
    pub fn drop_chunk(&self, seed: i32, chunk_x: i32, chunk_z: i32) {
        self.terrain.remove(seed, chunk_x, chunk_z);
    }

    /// TerraForged 6dd607eb's `getBaseHeight(x, z, type, ..)` at block `(x, z)` with the captured seed (Java's
    /// `Seeds.get(state)`, the same narrowed level seed): ONE un-eroded sample (`terrainCache.getSample`), its base
    /// (water) level and its raw height, each floored to a block; the surface types return the higher of the two plus
    /// one, the ocean-floor types the height plus one (Java `int` math). The height is not the eroded height the
    /// chunk is built to ([`Self::fill_from_noise`]), so on eroded ground it can differ from the filled surface, as in
    /// 6dd607eb (Reimagined reads `getErodedHeightNoise` here instead, so structures stand on the eroded ground; not
    /// followed). The result counts in TerraForged's levels (from block 0 up to `levels.max_y`) and is never clipped,
    /// unlike [`Self::fill_from_noise`], which clips the terrain to the chunk's build range: with the default levels
    /// it can name a height above 26.3's overworld top (320). Mapping it is the registering layer's job, together with
    /// the open world-height decision (widen, scale or clip). `Err` where the noise has no seed-first form (a custom
    /// module; see [`ErodedNoiseGenerator::noise_sample`]): the error is passed on rather than replaced by a height,
    /// and the caller decides (Java has no such failure).
    pub fn base_height(
        &self,
        x: i32,
        z: i32,
        heightmap: TerraHeightmap,
    ) -> Result<i32, &'static str> {
        let sample = self.terrain.noise_sample(self.seed(), x, z)?;
        Ok(base_height_from_noise(
            self.levels,
            sample.base_noise,
            sample.height_noise,
            heightmap,
        ))
    }

    /// `getBaseColumn(x, z, ..)` at block `(x, z)` with the captured seed, from the same un-eroded sample as
    /// [`Self::base_height`] (base level and raw height; 6dd607eb and Reimagined agree here). `Err` as
    /// [`Self::base_height`]. Panics
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
    override_biome: impl Fn(BiomeOverride) -> Arc<T>,
) -> Result<(), BiomeFillError<&'static str>> {
    fill_noise_biomes(chunk, position, |x, _y, z| {
        surface
            .sample_biome(seed, x.wrapping_shl(2), z.wrapping_shl(2), &override_biome)
            .map(|biome| T::clone(&biome))
    })
}

fn base_height_from_noise(
    levels: TerrainLevels,
    base_noise: f32,
    height_noise: f32,
    heightmap: TerraHeightmap,
) -> i32 {
    let base = levels.height(levels.scaled_base_level(base_noise));
    let height = levels.height(levels.scaled_height(height_noise));
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
        terraforged_current_surface_biomes::{
            NO_POOL_BIOME,
            tests::{NAMES, pools},
        },
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
    /// subnormals, huge values, infinities, NaN; negative heights throw). Given the noises, 6dd607eb's methods compute
    /// the same: `oracles/generator_facade/6dd607eb/out.txt` runs them verbatim (with its real `Seeds.get(RandomState)`)
    /// on a sample holding these noises and prints the same `S` and `m` lines.
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

    /// `GeneratorQueryOracle.CASES`/`POS` (= `HeightQueryOracle`'s): levels, terrain set, seed, droplets, lifetime, smoothing; block positions
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
    /// `oracles/generator_facade/6dd607eb/out.txt` `q` lines (`run.sh`; interpreted and JIT-compiled alike):
    /// TerraForged 6dd607eb's `getBaseHeight`/`getBaseColumn`, copied verbatim, with its real `Seeds.get(RandomState)`
    /// over the newer source's real `NoiseGenerator.getNoiseSample` (what 6dd607eb's `TerrainCache.getSample` reaches
    /// through `ErodedNoiseGenerator.getNoiseSample`); every query reads that sample once. Columns: case, x, z, base and
    /// height bits, surface and ocean-floor heights, `<column>`. The raw samples equal Reimagined's oracle
    /// (`../out.txt`), whose heights differ because it reads the eroded height.
    const JAVA_QUERIES: &str = "\
q 0 0 0 3df5fc22 3e339f1c 85 85 84 84 84 0
q 0 5 77 3e0dc23c 3f2c6fd0 324 324 323 323 323 0
q 0 -1 -1 3df5fc22 3e489a6e 95 95 94 94 94 0
q 0 -123 -51 3dc97019 3f57f5c3 405 405 404 404 404 0
q 1 2147483647 -2147483648 00000000 3d9c28f6 63 37 36 62 36 26
q 1 2147483638 -2147483645 00000000 3d9c28f6 63 37 36 62 36 26
q 1 -2147483648 2147483647 00000000 3d9c28f6 63 37 36 62 36 26
q 2 1000 -1000 3ece3552 3e612d81 114 106 105 113 105 8
q 2 -30000000 29999999 3ee64c34 3e9ab620 146 146 145 145 145 0
q 2 -29999990 29999950 3ee50bda 3e813107 122 122 121 121 121 0
q 3 300 300 3e9f02ef 3f0ee7e3 143 143 142 142 142 0
q 3 -777 1234 3eae8d03 3f0d3be1 142 142 141 141 141 0
q 3 -770 1240 3eafbae6 3f060470 135 135 134 134 134 0
";

    /// End to end against 6dd607eb's Java (`q` lines): the facade's `base_height`/`base_column` through its terrain
    /// cache, with the seed captured from a level seed whose low 32 bits are the case's seed. All six heightmap types
    /// are asked. The cases include eroded ground where the eroded height (what the chunk is filled to) would give a
    /// different answer, so reading it instead of the raw sample fails here.
    #[test]
    fn queries_match_6dd607eb_java() {
        let mut out = Vec::new();
        let mut eroded_differs = false;
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
                let ocean = g.base_height(x, z, TerraHeightmap::OceanFloor).unwrap();
                for kind in [
                    TerraHeightmap::WorldSurfaceWorldgen,
                    TerraHeightmap::MotionBlocking,
                    TerraHeightmap::MotionBlockingNoLeaves,
                ] {
                    assert_eq!(g.base_height(x, z, kind).unwrap(), surface);
                }
                assert_eq!(
                    g.base_height(x, z, TerraHeightmap::OceanFloorWorldgen)
                        .unwrap(),
                    ocean
                );
                eroded_differs |= ocean
                    != base_height_from_noise(
                        g.levels(),
                        sample.base_noise,
                        eroded,
                        TerraHeightmap::OceanFloor,
                    );
                let col = g.base_column(x, z).unwrap();
                assert_eq!(
                    col,
                    base_column_from_noise(g.levels(), sample.base_noise, sample.height_noise)
                );
                out.push(format!(
                    "q {case} {x} {z} {} {} {surface} {ocean} {}",
                    h(sample.base_noise),
                    h(sample.height_noise),
                    column(g.levels(), sample.base_noise, sample.height_noise)
                ));
            }
        }
        assert_eq!(out, JAVA_QUERIES.lines().collect::<Vec<_>>());
        assert!(
            eroded_differs,
            "the cases must separate the raw height from the eroded one"
        );
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

    /// The surface cache reads the shared entry's current heights (structure edits included) with the captured seed,
    /// pads the 3x3 chunk area with their minimum, and leaves a filled cache alone.
    #[test]
    fn surface_cache_reads_the_shared_edited_heights() {
        use super::super::terraforged_surface_height_cache::column_key;
        let g = facade(0, 0, 0, 25, 0);
        g.capture_seed((5_i64 << 32) | 7);
        let data = g.chunk_data(7, -1, 2);
        {
            let mut terrain = data.write();
            for z in 0..16 {
                for x in 0..16 {
                    terrain.set_height_at(x, z, 70 + x - z * 2);
                }
            }
        }
        let mut cache = BTreeMap::new();
        g.initialize_surface_cache(ChunkPos::new(-1, 2), &mut cache);
        assert_eq!(cache.len(), 144);
        assert_eq!(cache[&column_key(-16, 32)], 70);
        assert_eq!(cache[&column_key(-4, 44)], 70 + 12 - 24);
        assert_eq!(cache[&column_key(-32, 32)], 46);
        assert_eq!(cache[&column_key(12, 60)], 46);
        assert_eq!(cache[&column_key(-32, 16)], 46);
        assert!(!cache.contains_key(&column_key(16, 32)));
        data.write().set_height_at(0, 0, 200);
        g.initialize_surface_cache(ChunkPos::new(-1, 2), &mut cache);
        assert_eq!(cache[&column_key(-16, 32)], 70);
        // A newly captured seed reads its own entry, not the edited one.
        let mut other = BTreeMap::new();
        g.capture_seed(8);
        g.initialize_surface_cache(ChunkPos::new(-1, 2), &mut other);
        let own = g.chunk_data(8, -1, 2).read().height_at(0, 0);
        assert_ne!(own, 200);
        assert_eq!(other[&column_key(-16, 32)], own);
        g.drop_chunk(7, -1, 2);
        g.drop_chunk(8, -1, 2);
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
        let data = terrain.read();
        for z in 0..16 {
            for x in 0..16 {
                let height = data.height_at(x, z);
                let water = TerrainLevels::water_level(x, z, g.sea_level(), &data);
                let top = height.max(water);
                assert_eq!(chunk.block(x, height, z), STONE);
                assert_eq!(
                    chunk.block(x, top, z),
                    if top == height { STONE } else { WATER }
                );
                assert_eq!(chunk.block(x, top + 1, z), AIR);
            }
        }
        drop(data);
        g.drop_chunk(7, -2, 3);
        assert!(!terrain.ptr_eq(&g.chunk_data(7, -2, 3)));
    }

    /// Every column's height and every block and heightmap of `chunk` (for comparing whole terrain stages).
    fn stage_state(
        terrain: &TerrainDataHandle<ErodedNoiseGenerator>,
        chunk: &TerrainChunk<'_, Properties>,
    ) -> (Vec<i32>, u64) {
        let data = terrain.read();
        let heights = (0..16)
            .flat_map(|z| (0..16).map(move |x| (x, z)))
            .map(|(x, z)| data.height_at(x, z))
            .collect();
        (heights, digest(chunk))
    }

    /// The structure stage is `fill_chunk` then `modify_chunk` on the entry's own `TerrainData` (6dd607eb's
    /// `fillFromNoise`): the chunk at its position gets the structure's blocks and OCEAN_FLOOR_WG, and every handle of
    /// the entry the edited heights, with the extrema unchanged. Running a stage again for the same entry starts from
    /// the generated heights: the structure stage gives the same chunk and heights, the plain fill the plain ones.
    #[test]
    fn structure_stage_shares_its_heights_and_reruns_from_the_generated_terrain() {
        use super::super::terraforged_structure_terrain::{Bounds, Piece};
        use crate::heightmap::HeightmapType;

        let g = facade(0, 0, 0, 25, 0);
        g.capture_seed((5_i64 << 32) | 7);
        let blocks = registry();
        let position = ChunkPos::new(-2, 3);
        let new_chunk = || TerrainChunk::new(position, -64, 544, &blocks, Properties).unwrap();
        let cached = g.chunk_data(7, -2, 3);
        let extrema = |terrain: &TerrainDataHandle<ErodedNoiseGenerator>| {
            let data = terrain.read();
            (data.min(), data.max(), data.max_base())
        };
        let generated = extrema(&cached);
        let floor = generated.1 + 10;
        // Over block x -32..=-20, z 48..=60 of chunk (-2, 3), above all its terrain.
        let structures = StructureTerrain::from_rigid_pieces(vec![Piece {
            bounds: Bounds {
                min_x: -32,
                min_y: floor,
                min_z: 48,
                max_x: -20,
                max_y: floor + 5,
                max_z: 60,
            },
            ground_delta: 0,
        }]);

        let mut plain = new_chunk();
        let plain_terrain = g.fill_from_noise(&mut plain, STONE, WATER).unwrap();
        let plain_state = stage_state(&plain_terrain, &plain);

        let mut chunk = new_chunk();
        let terrain = g
            .fill_from_noise_with_structures(&mut chunk, STONE, WATER, AIR, &structures)
            .unwrap();
        assert!(terrain.ptr_eq(&cached));
        let state = stage_state(&terrain, &chunk);
        assert_ne!(state, plain_state);
        // Block (-30, 50) is chunk column (2, 2): raised to the piece's floor, air from there.
        assert_eq!(cached.read().height_at(-30, 50), floor - 1);
        assert_eq!(chunk.block(2, floor - 1, 2), STONE);
        assert_eq!(chunk.block(2, floor, 2), AIR);
        assert_eq!(
            chunk
                .heightmap(HeightmapType::OceanFloorWg)
                .unwrap()
                .highest_taken(2, 2)
                .ok(),
            Some(floor - 1)
        );
        assert_eq!(extrema(&g.chunk_data(7, -2, 3)), generated);

        let mut again = new_chunk();
        let rerun = g
            .fill_from_noise_with_structures(&mut again, STONE, WATER, AIR, &structures)
            .unwrap();
        assert!(rerun.ptr_eq(&cached));
        assert_eq!(stage_state(&rerun, &again), state);
        let mut plain_again = new_chunk();
        let plain_rerun = g.fill_from_noise(&mut plain_again, STONE, WATER).unwrap();
        assert_eq!(stage_state(&plain_rerun, &plain_again), plain_state);

        // Concurrent runs serialize and leave the same chunk and heights.
        thread::scope(|scope| {
            let runs: Vec<_> = (0..4)
                .map(|_| {
                    scope.spawn(|| {
                        let mut chunk = new_chunk();
                        let terrain = g
                            .fill_from_noise_with_structures(
                                &mut chunk,
                                STONE,
                                WATER,
                                AIR,
                                &structures,
                            )
                            .unwrap();
                        digest(&chunk) == state.1 && terrain.ptr_eq(&cached)
                    })
                })
                .collect();
            assert!(runs.into_iter().all(|run| run.join().unwrap()));
        });
        assert_eq!(stage_state(&cached, &chunk), state);

        // The same two steps by hand on a freshly generated entry.
        g.drop_chunk(7, -2, 3);
        let fresh = g.chunk_data(7, -2, 3);
        assert!(!fresh.ptr_eq(&cached));
        let mut direct = new_chunk();
        {
            let mut data = fresh.write();
            fill_chunk(g.sea_level(), &mut direct, &data, STONE, WATER).unwrap();
            structures
                .modify_chunk(&mut direct, &mut data, STONE, AIR)
                .unwrap();
        }
        assert_eq!(stage_state(&fresh, &direct), state);
        g.drop_chunk(7, -2, 3);
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
                let line = {
                    let data = terrain.read();
                    format!(
                        "t {case} {cx} {cz} {} {} {}",
                        data.min(),
                        data.max(),
                        data.max_base()
                    )
                };
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

    /// The pools of [`create_biomes_hints_and_fills_one_layer`]: every climate type gets one biome.
    fn one(biome: &Arc<&'static str>, weight: f32) -> BiomePools<&'static str> {
        let mut pools = BiomePools::default();
        for climate in BiomeType::VALUES {
            pools.insert(
                climate,
                Arc::new(WeightMap::new(vec![Arc::clone(biome)], &[weight])),
            );
        }
        pools
    }

    fn named(replacement: BiomeOverride) -> Arc<&'static str> {
        Arc::new(replacement.key().unwrap())
    }

    /// `createBiomes` hints the chunk with the captured (narrowed) seed, then fills it at that seed with each column's
    /// surface biome at every height (no cave layer); a sampler built over another generator is refused; a failing
    /// surface sample is an `Err` that writes nothing.
    #[test]
    fn create_biomes_hints_and_fills_one_layer() {
        let g = facade(0, 0, 0, 25, 0);
        g.capture_seed((5_i64 << 32) | 7);
        let land = Arc::new("land");
        let pools = one(&land, 1.0);
        let plains = Arc::new("plains");
        let surface = g.surface_biome_sampler(&pools, &plains);
        let first = ChunkPos::new(-3, 5);
        for position in [first, ChunkPos::new(0, 0), ChunkPos::new(97, -61)] {
            let mut chunk = BiomeChunk::new(position, -64, 384, "").unwrap();
            assert!(g.terrain.is_empty());
            g.create_biomes(&mut chunk, position, &surface, named)
                .unwrap();
            // Hinted with `Seeds.get` of the level seed (7), not the raw level seed.
            assert_eq!(g.terrain.len(), 1);
            g.drop_chunk(7, position.x, position.z);
            assert!(g.terrain.is_empty());

            let mut direct = BiomeChunk::new(position, -64, 384, "").unwrap();
            fill_biomes(7, &mut direct, position, &surface, named).unwrap();
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
                    .sample_biome(7, column[0] << 2, column[2] << 2, named)
                    .unwrap();
                for y in [-16, -1, 0, 16, 79] {
                    assert_eq!(*chunk.noise_biome([column[0], y, column[2]]), *expected);
                }
            }
        }

        let other = facade(0, 0, 0, 25, 0);
        let foreign = other.surface_biome_sampler(&pools, &plains);
        let mut chunk = BiomeChunk::new(first, -64, 384, "").unwrap();
        assert!(
            catch_unwind(AssertUnwindSafe(|| {
                g.create_biomes(&mut chunk, first, &foreign, named)
            }))
            .is_err()
        );

        // A pool with no biome for any noise (all weights zero; Java's `getValue` returns null): the first land
        // column fails the fill before anything is written.
        let null = one(&land, 0.0);
        let surface = g.surface_biome_sampler(&null, &plains);
        let mut chunk = BiomeChunk::new(first, -64, 384, "unset").unwrap();
        assert!(matches!(
            fill_biomes(7, &mut chunk, first, &surface, named),
            Err(BiomeFillError::Resolve(NO_POOL_BIOME))
        ));
        assert!((0..24).all(|i| chunk.section(i).unwrap().iter().all(|b| *b == "unset")));
    }

    /// `.local/terraforged/oracles/chunk_biomes/6dd607eb/` (`run.sh`, after `../run.sh`, which needs
    /// `../noise_generator/run.sh`): `../run.sh`'s half A records what `sampleBiome` reads at every quart column of
    /// [`CHUNKS`] from 6dd607eb's real `Sampler` and the newer source's `NoiseGenerator`; `6dd607eb/run.sh` runs
    /// TerraForged 6dd607eb's `ChunkUtil.fillNoiseBiomes` and `Source` members (verbatim) with its real `BiomeBuffer2D`
    /// over 6dd607eb's real `BiomeSampler`, `IBiomeSampler.Sampler` and `ClimateNoise` (on the Engine 0.3.0 class
    /// path of `.local/terraforged/oracles/climate_noise/`) on that record, into a stub of 1.19's chunk sections,
    /// interpreted and with `-Xcomp`. Per levels variant, terrain set and configuration `k`
    /// (`current_surface_biomes`' pools): the chunk count and a fold of every cell's biome name of every
    /// chunk in each of [`RANGES`], over the six runtime seeds (level seeds `0x5eed << 32 | seed`); then every name's
    /// cell count. The chunks go through [`fill_biomes`] at the captured seed: `create_biomes` without its hint, which
    /// would start a region build per chunk ([`create_biomes_hints_and_fills_one_layer`] checks that the two fill
    /// alike). None of these columns falls in the narrow (0.5, 0.505] beach band, so no beach appears (the
    /// override's beach band is checked by `biome_sampler`'s and `current_surface_biomes`' oracles).
    const JAVA_CHUNK_BIOMES: &str = "\
chunks 0 0 0 612 a1a8b84ecc77d9f1\n\
chunks 0 0 1 612 add03f6006e789d1\n\
chunks 0 0 2 612 2752458a9ff0db11\n\
chunks 0 0 3 612 6f56103f2307ed91\n\
chunks 0 1 0 612 a1a8b84ecc77d9f1\n\
chunks 0 1 1 612 add03f6006e789d1\n\
chunks 0 1 2 612 2752458a9ff0db11\n\
chunks 0 1 3 612 6f56103f2307ed91\n\
chunks 1 0 0 612 a1a8b84ecc77d9f1\n\
chunks 1 0 1 612 add03f6006e789d1\n\
chunks 1 0 2 612 2752458a9ff0db11\n\
chunks 1 0 3 612 6f56103f2307ed91\n\
chunks 1 1 0 612 a1a8b84ecc77d9f1\n\
chunks 1 1 1 612 add03f6006e789d1\n\
chunks 1 1 2 612 2752458a9ff0db11\n\
chunks 1 1 3 612 6f56103f2307ed91\n\
chunks 2 0 0 612 88a5d598074bf0f1\n\
chunks 2 0 1 612 1b3d66e9707b5471\n\
chunks 2 0 2 612 132d26863b164471\n\
chunks 2 0 3 612 1f169b321d9e36d1\n\
chunks 2 1 0 612 88a5d598074bf0f1\n\
chunks 2 1 1 612 1b3d66e9707b5471\n\
chunks 2 1 2 612 132d26863b164471\n\
chunks 2 1 3 612 1f169b321d9e36d1\n\
names birch_forest=2222848 desert=2261760 forest=724736 grove=2062336 jungle=3202944 meadow=510720 minecraft:deep_cold_ocean=2869760 minecraft:deep_frozen_ocean=7626752 minecraft:deep_lukewarm_ocean=3618816 minecraft:deep_ocean=5651968 minecraft:frozen_river=1400832 minecraft:river=5992448 plains=1945600 snowy_plains=1568640 swamp=2699520 taiga=1303552 windswept_hills=1965056\n";

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
    const SEEDS: [i32; 6] = [0, 42, -17, i32::MAX, i32::MIN, -8_763_214];

    fn chunk_biome_lines() -> Vec<String> {
        let palette: Vec<Arc<&'static str>> = NAMES.iter().map(|name| Arc::new(*name)).collect();
        let plains = Arc::new("plains");
        let configs: Vec<_> = (0..4).map(|k| pools(&palette, k)).collect();
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
                    for (k, pools) in configs.iter().enumerate() {
                        let surface = g.surface_biome_sampler(pools, &plains);
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
    fn chunk_biomes_match_terraforged_java() {
        let expected: Vec<&str> = JAVA_CHUNK_BIOMES.lines().collect();
        let actual = chunk_biome_lines();
        assert_eq!(actual.len(), expected.len());
        for (actual, expected) in actual.iter().zip(expected) {
            assert_eq!(actual, expected);
        }
    }
}
