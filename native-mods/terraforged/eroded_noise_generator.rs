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
// `buildRegion`, `erodedHeightNoise`, `erodedHeight`, `getRegion`, `generate`, `find`, `getNoiseSample`,
// `sample`, `getHeightNoise`, `getLevels` and `getTerrainLevels` of
// `src/main/java/com/terraforged/mod/worldgen/noise/erosion/ErodedNoiseGenerator.java`. MIT License, Copyright (c)
// 2020 TerraForged, Copyright (c) 2026 Romoslayer; see `LICENSE-TERRAFORGED-REIMAGINED`.

use super::{
    terraforged_engine::Terrain,
    terraforged_erosion::{
        ErosionFilter, ErosionFilterSettings, ErosionRegionCache, ErosionRegionLayout,
        ErosionResource, HeightmapSmoothing, HeightmapSmoothingSettings,
    },
    terraforged_noise_generator::NoiseGenerator,
    terraforged_terrain_data::{NoiseData, NoiseSample},
    terraforged_terrain_generator::NoiseGenerator as NoiseGeneratorContract,
    terraforged_terrain_levels::{NoiseLevels, TerrainLevels},
};
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    sync::Arc,
};

/// TerraForged Reimagined 26.3's `ErodedNoiseGenerator` (Romoslayer/TerraForged-Reimagined a24d450): erosion runs
/// once per [`ErosionRegionLayout`] region (8x8 chunks plus 4 chunks of margin, a 256x256 map) over the raw terrain
/// heights of the wrapped [`NoiseGenerator`], then the optional [`HeightmapSmoothing`], and the result is cached per
/// region and seed ([`ErosionRegionCache`], Java's `getRegion`). Neither the 1.18.2-0.3.1 jar nor the newer source
/// (6dd607eb) has regions: they erode a `NoiseTileSize` tile per chunk. Reimagined's
/// `NoiseGenerator.sampleTerrain`/`sampleRiver` are 6dd607eb's, which [`NoiseGenerator`] ports. Chunks are filled
/// from the regions ([`Self::generate`]); the plain height and `find` stay on the un-eroded generator.
pub struct ErodedNoiseGenerator {
    erosion: ErosionFilter,
    generator: NoiseGenerator,
    smoothing: HeightmapSmoothingSettings,
    regions: ErosionRegionCache<ErodedRegion>,
    river_start: RiverStart,
    deposits: bool,
}

/// One cached erosion region: the eroded heights (`buildRegion`'s map, which indexing reads) and, with the
/// `erosion_deposits` setting (Horizon's, not Java's), erosion's change to each block of the map: its height noise
/// after erosion (and smoothing, off in TerraForged's presets) minus its raw height noise.
pub struct ErodedRegion {
    heights: Vec<f32>,
    change: Option<Vec<f32>>,
}

impl std::ops::Index<usize> for ErodedRegion {
    type Output = f32;

    fn index(&self, index: usize) -> &f32 {
        &self.heights[index]
    }
}

/// What every chunk sample's river noise starts at in [`ErodedNoiseGenerator::generate`], before the rivers carve it.
/// 6dd607eb keeps one `NoiseResource` per worker thread and reuses its 18x18 samples for every chunk the thread
/// generates, never resetting their river noise, which the carver only multiplies (by factors in 0..=1 inside a valley)
/// and sets back to 1 only seaward of the beach. So a sample starts where the same position of the thread's previous
/// chunk left it: 1 in a new resource, falling with every valley it has passed through, and exactly 0, for good, once
/// one of them was a river bed. A few chunks into play nearly every land sample is at 0 or within a few hundredths of
/// it (replayed on seed 777: 98.5% of the land samples of 625 chunks around spawn on six workers, every one from a
/// worker's third chunk on); only a worker's first chunks, its first after the sea, or a worker starting far from rivers
/// (sparse for a few dozen chunks) differ, and those depend on scheduling.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RiverStart {
    /// The default: the 0 Java's reused samples settle at in play (TerraForged's dense forests), the same for every
    /// chunk, whatever the threads or the order.
    InPlay,
    /// A new resource's 1 (`NoiseSample`'s default), what TerraForged's code intends: the `sparse_forests` setting,
    /// and what the Java oracles replay (they start every chunk from a new resource).
    Fresh,
}

impl RiverStart {
    /// The value a sample's river noise starts at.
    pub const fn value(self) -> f32 {
        match self {
            Self::InPlay => 0.0,
            Self::Fresh => 1.0,
        }
    }

    /// The world's choice: [`Self::Fresh`] with `sparse_forests`, else [`Self::InPlay`].
    pub fn for_settings(settings: super::terraforged_settings::Settings) -> Self {
        if settings.sparse_forests {
            Self::Fresh
        } else {
            Self::InPlay
        }
    }
}

impl ErodedNoiseGenerator {
    /// `new ErodedNoiseGenerator(generator, filters)`: `filters.erosion`'s six values become the region-wide
    /// [`ErosionFilter`]'s settings on a [`ErosionRegionLayout::MAP_SIZE`] map; `filters.smoothing` is kept as is.
    /// Chunk samples' river noise starts as the world's settings say ([`RiverStart::for_settings`]).
    pub fn new(
        generator: NoiseGenerator,
        erosion: ErosionFilterSettings,
        smoothing: HeightmapSmoothingSettings,
    ) -> Self {
        Self {
            erosion: ErosionFilter::new(ErosionRegionLayout::MAP_SIZE, erosion),
            generator,
            smoothing,
            regions: ErosionRegionCache::new(),
            river_start: RiverStart::for_settings(super::terraforged_settings::current()),
            deposits: super::terraforged_settings::current().erosion_deposits,
        }
    }

    /// The same generator recording erosion's deposits in its chunks ([`Self::generate`]) or not.
    pub fn with_deposits(mut self, deposits: bool) -> Self {
        self.deposits = deposits;
        self
    }

    /// Blocks around a column (in a square, 7x7) whose mean change from erosion the column's own is measured against
    /// for `erosion_deposits`: erosion lowers nearly all ground a little, so sediment is where a column lost less than
    /// the ground around it, or gained.
    pub const DEPOSIT_RADIUS: i32 = 3;

    /// The same generator with chunk samples' river noise starting at `start`.
    pub fn with_river_start(mut self, start: RiverStart) -> Self {
        self.river_start = start;
        self
    }

    /// What chunk samples' river noise starts at.
    pub fn river_start(&self) -> RiverStart {
        self.river_start
    }

    /// How many eroded regions are cached.
    #[cfg(test)]
    pub(crate) fn cached_regions(&self) -> usize {
        self.regions.len()
    }

    /// The wrapped, un-eroded generator (Java's `generator` field).
    pub fn generator(&self) -> &NoiseGenerator {
        &self.generator
    }

    /// `getNoiseSample(seed, x, z)`: the un-eroded generator's sample of the block (its `getNoiseSample`, a reset
    /// sample filled by `sample`), without erosion. Java returns its thread-local sample; this returns a new one.
    /// `Err` where [`NoiseGenerator::sample`] fails.
    pub fn noise_sample(&self, seed: i32, x: i32, z: i32) -> Result<NoiseSample, &'static str> {
        let mut sample = NoiseSample::new(&self.generator.continent().context().terrain);
        self.sample(seed, x, z, &mut sample)?;
        Ok(sample)
    }

    /// `sample(seed, x, z, sample)`: the un-eroded generator's [`NoiseGenerator::sample`] into `sample` (not reset
    /// first, as in Java), with a new blender. `Err` where that fails.
    pub fn sample(
        &self,
        seed: i32,
        x: i32,
        z: i32,
        sample: &mut NoiseSample,
    ) -> Result<(), &'static str> {
        let mut blender = self.generator.blender_resource();
        self.generator.sample(seed, x, z, sample, &mut blender)?;
        Ok(())
    }

    /// `erodedHeightNoise(seed, x, z)`: the column's height as the world is built, erosion and rivers included. A new
    /// sample gets the raw terrain at the block's noise coordinates (`sampleTerrain`, for the continent and terrain
    /// fields), then its height is replaced by the eroded region's (building the region if no cached one has this
    /// seed), then the rivers carve it (`sampleRiver`). `Err` where the terrain sample or the region build fails (a
    /// custom module, see [`NoiseGenerator::sample`]), where Java throws; a failed build is not cached.
    pub fn eroded_height_noise(&self, seed: i32, x: i32, z: i32) -> Result<f32, &'static str> {
        let mut sample = NoiseSample::new(&self.generator.continent().context().terrain);
        let noise_x = self.noise_coord(x);
        let noise_z = self.noise_coord(z);
        let mut blender = self.generator.blender_resource();
        self.generator
            .sample_terrain(seed, noise_x, noise_z, &mut sample, &mut blender)?;
        sample.height_noise = self.eroded_height(seed, x, z)?;
        Ok(self
            .generator
            .sample_river(seed, noise_x, noise_z, &mut sample)
            .height_noise)
    }

    /// `erodedHeight(seed, x, z)`: the block's height in its cached region ([`ErosionRegionLayout::block_index`]).
    fn eroded_height(&self, seed: i32, x: i32, z: i32) -> Result<f32, &'static str> {
        let region = self.region(seed, x >> 4, z >> 4)?;
        Ok(region[ErosionRegionLayout::block_index(x, z)])
    }

    /// `getRegion(seed, chunkX, chunkZ)`: the eroded map of the chunk's region, built on this thread by
    /// [`Self::build_region`] when the cache has none for this seed.
    fn region(
        &self,
        seed: i32,
        chunk_x: i32,
        chunk_z: i32,
    ) -> Result<Arc<ErodedRegion>, &'static str> {
        self.regions
            .try_get_or_build(seed, chunk_x, chunk_z, |region_x, region_z| {
                self.build_region(seed, region_x, region_z)
            })
    }

    /// `generate(seed, chunkX, chunkZ, consumer)`: fills one chunk's [`NoiseData`] (the chunk and its one-block border,
    /// `dz` then `dx` from `NoiseData::MIN` to `MAX - 1`, at `(chunk << 4) + d`, `int` wrap) and hands it to `consumer`
    /// once. Each position takes the base, continent and terrain fields from the raw terrain sample
    /// ([`NoiseGenerator::sample_terrain`]), its height from the chunk's eroded region (the region's map at
    /// [`ErosionRegionLayout::chunk_offset`] `+ d`: a border position reads the region's own margin, not the
    /// neighbouring region, as in Java, so it can differ from the neighbouring chunk's edge), then the rivers carve it
    /// ([`NoiseGenerator::sample_river`]).
    ///
    /// Java fills every position into its own `NoiseSample` of a pooled `NoiseResource`, so a new resource starts each
    /// position from the defaults (river noise 1), while a reused one carries the river noise the carver multiplies
    /// over from the same position of whichever chunk used that resource before ([`RiverStart`]). The port resets the
    /// sample at every position and starts its river noise at [`Self::river_start`]: the 0 reused samples settle at
    /// in play by default, a new resource's 1 with `sparse_forests` (and for the Java oracles). Either way a chunk's
    /// data never depends on the chunks generated before it or on which thread generated them; it also fills a new
    /// `NoiseData` per call, whose own `sample` stays the defaults (Java never writes it here).
    ///
    /// `Err` (and no `consumer` call) where the region build fails, where Java's `getRegion` throws out of `generate`.
    /// As in Java, a failure after the region is read (a failing sample, or a panic, the consumer's included) is not
    /// passed on: it is logged as "Eroded noise failed for chunk x, z -- that chunk will be left unfilled" and the call
    /// returns `Ok`, the consumer not called where the failure came before it.
    pub fn generate(
        &self,
        seed: i32,
        chunk_x: i32,
        chunk_z: i32,
        consumer: impl FnOnce(&NoiseData),
    ) -> Result<(), &'static str> {
        let region = self.region(seed, chunk_x, chunk_z)?;
        let offset_x = ErosionRegionLayout::chunk_offset(chunk_x);
        let offset_z = ErosionRegionLayout::chunk_offset(chunk_z);
        let filled = catch_unwind(AssertUnwindSafe(|| -> Result<(), &'static str> {
            let terrains = &self.generator.continent().context().terrain;
            let mut chunk = NoiseData::new(terrains);
            let mut sample = NoiseSample::new(terrains);
            let mut blender = self.generator.blender_resource();
            let start_x = chunk_x.wrapping_shl(4);
            let start_z = chunk_z.wrapping_shl(4);
            for dz in NoiseData::MIN..NoiseData::MAX {
                let noise_z = self.noise_coord(start_z.wrapping_add(dz));
                let row = offset_z
                    .wrapping_add(dz)
                    .wrapping_mul(ErosionRegionLayout::MAP_SIZE)
                    .wrapping_add(offset_x);
                for dx in NoiseData::MIN..NoiseData::MAX {
                    let noise_x = self.noise_coord(start_x.wrapping_add(dx));
                    sample.reset(terrains);
                    sample.river_noise = self.river_start.value();
                    self.generator.sample_terrain(
                        seed,
                        noise_x,
                        noise_z,
                        &mut sample,
                        &mut blender,
                    )?;
                    sample.height_noise = region[row.wrapping_add(dx) as usize];
                    self.generator
                        .sample_river(seed, noise_x, noise_z, &mut sample);
                    chunk.set_noise(dx, dz, &sample);
                    if let Some(change) = &region.change {
                        // Not Java's (`erosion_deposits`): the column's change from erosion against the mean of
                        // the 7x7 around it, all read from the region's map, so neighbouring chunks agree.
                        chunk.set_deposit(
                            dx,
                            dz,
                            relative_change(change, offset_x + dx, offset_z + dz),
                        );
                    }
                }
            }
            consumer(&chunk);
            Ok(())
        }));
        let cause = match filled {
            Ok(Ok(())) => return Ok(()),
            Ok(Err(error)) => error,
            Err(_) => "panicked",
        };
        eprintln!(
            "Eroded noise failed for chunk {chunk_x}, {chunk_z} -- that chunk will be left unfilled: {cause}"
        );
        Ok(())
    }

    /// `find(seed, x, z, minRadius, maxRadius, terrain)`: the un-eroded generator's [`NoiseGenerator::find`].
    pub fn find(
        &self,
        seed: i32,
        x: i32,
        z: i32,
        min_radius: i32,
        max_radius: i32,
        terrain: &Terrain,
    ) -> Result<i64, &'static str> {
        self.generator
            .find(seed, x, z, min_radius, max_radius, terrain)
    }

    /// `getNoiseCoord(coord)`: `coord * frequency` (int to float, then a float multiply).
    fn noise_coord(&self, coord: i32) -> f32 {
        coord as f32 * self.generator.levels().noise_levels.frequency
    }

    /// `buildRegion(seed, regionX, regionZ)`: the raw terrain height (`sampleTerrain`, one sample reused for the whole
    /// map as in Java; only its height is read) of every block of the region's map, rows (`z`) then columns, from the
    /// origin chunk's first block (`int` wrap), then [`ErosionFilter::apply_region`] over its
    /// [`ErosionRegionLayout::TOTAL_CHUNKS`] chunks from the origin chunk, then [`HeightmapSmoothing::apply`]. `Err`
    /// on the first failing sample. With `erosion_deposits` (Horizon's) the region also keeps erosion's change to
    /// every block of the map (the map after erosion and smoothing minus the raw heights); the heights are the same
    /// either way.
    fn build_region(
        &self,
        seed: i32,
        region_x: i32,
        region_z: i32,
    ) -> Result<ErodedRegion, &'static str> {
        let (origin_chunk_x, origin_chunk_z) =
            ErosionRegionLayout::origin_chunk(region_x, region_z);
        let start_x = origin_chunk_x.wrapping_shl(4);
        let start_z = origin_chunk_z.wrapping_shl(4);
        let map_size = ErosionRegionLayout::MAP_SIZE;
        let mut map = Vec::with_capacity((map_size * map_size) as usize);
        let mut sample = NoiseSample::new(&self.generator.continent().context().terrain);
        let mut blender = self.generator.blender_resource();
        for z in 0..map_size {
            let noise_z = self.noise_coord(start_z.wrapping_add(z));
            for x in 0..map_size {
                let noise_x = self.noise_coord(start_x.wrapping_add(x));
                let height = self
                    .generator
                    .sample_terrain(seed, noise_x, noise_z, &mut sample, &mut blender)?
                    .height_noise;
                map.push(height);
            }
        }
        let raw = self.deposits.then(|| map.clone());
        self.erosion.apply_region(
            seed,
            origin_chunk_x,
            origin_chunk_z,
            ErosionRegionLayout::TOTAL_CHUNKS,
            map_size,
            &mut ErosionResource::default(),
            &mut map,
        );
        HeightmapSmoothing::apply(&mut map, map_size, self.smoothing);
        let change = raw.map(|raw| {
            map.iter()
                .zip(raw)
                .map(|(eroded, raw)| eroded - raw)
                .collect()
        });
        Ok(ErodedRegion {
            heights: map,
            change,
        })
    }
}

/// The change at map block `(x, z)` minus the mean change of the square of [`ErodedNoiseGenerator::DEPOSIT_RADIUS`]
/// around it (a chunk's blocks and border sit at least 60 blocks inside the map, so the square never leaves it).
fn relative_change(change: &[f32], x: i32, z: i32) -> f32 {
    let size = ErosionRegionLayout::MAP_SIZE;
    let radius = ErodedNoiseGenerator::DEPOSIT_RADIUS;
    let mut sum = 0.0f32;
    for nz in z - radius..=z + radius {
        for nx in x - radius..=x + radius {
            sum += change[(nz * size + nx) as usize];
        }
    }
    let side = (2 * radius + 1) as f32;
    change[(z * size + x) as usize] - sum / (side * side)
}

/// Reimagined's `INoiseGenerator` methods that `TerrainGenerator` calls. `getLevels`/`getTerrainLevels` and
/// `getHeightNoise` are the un-eroded generator's (Reimagined keeps the per-biome-sample height off the region cache on
/// purpose); `generate` is [`ErodedNoiseGenerator::generate`], which panics where it returns `Err` (a failed region
/// build, where Java throws; TerraForged's graphs have no custom module, so this never happens for them).
impl NoiseGeneratorContract for ErodedNoiseGenerator {
    fn levels(&self) -> NoiseLevels {
        NoiseGeneratorContract::levels(&self.generator)
    }

    fn terrain_levels(&self) -> TerrainLevels {
        self.generator.levels()
    }

    fn height_noise(&self, seed: i32, x: i32, z: i32) -> f32 {
        NoiseGeneratorContract::height_noise(&self.generator, seed, x, z)
    }

    fn generate(
        &self,
        seed: i32,
        chunk_x: i32,
        chunk_z: i32,
        consumer: &mut dyn FnMut(&NoiseData),
    ) {
        ErodedNoiseGenerator::generate(self, seed, chunk_x, chunk_z, consumer)
            .unwrap_or_else(|error| panic!("ErodedNoiseGenerator.generate: {error}"));
    }
}

#[cfg(test)]
mod tests {
    use super::super::{
        terraforged::Module,
        terraforged_noise_generator::tests::{Fold, generator, generator_with},
        terraforged_terrain_blender::TerrainNoise,
    };
    use super::*;
    use std::thread;

    const _: () = {
        const fn send_sync<T: Send + Sync>() {}
        send_sync::<ErodedNoiseGenerator>();
    };

    /// `oracles/erosion_region/e2e/out.txt` (`run.sh`): Reimagined's own `buildRegion` over the newer source's
    /// `NoiseGenerator`, interpreted and JIT-compiled alike. Cases: levels, terrain set, seed, region, droplets,
    /// lifetime, smoothing iterations (as in `ErodedRegionOracle.CASES`).
    const CASES: [(usize, usize, i32, i32, i32, i32, i32, i32); 6] = [
        (0, 1, 0, 0, 0, 350, 25, 0),
        (0, 1, 0, -1, -1, 350, 25, 0),
        (0, 1, 42, 2, -3, 350, 25, 0),
        (0, 1, -17, -1, 0, 350, 25, 2),
        (2, 1, 42, 0, -1, 350, 25, 1),
        (0, 0, i32::MIN, 1, 1, 100, 12, 0),
    ];
    const BLOCKS: [(i32, i32); 4] = [(0, 0), (127, 127), (5, 77), (64, 3)];
    const JAVA: &str = "\
r 0 0 1 0 0 0 e064fda469f5a5a5
h 0 0 0 3e7e455f
h 0 127 127 3f1ec0d1
h 0 5 77 3f29702c
h 0 64 3 3ebb2ba1
r 1 0 1 0 -1 -1 d3f5a54dfe0dc56b
h 1 -128 -128 3f3ec94f
h 1 -1 -1 3e7e7eab
h 1 -123 -51 3f54c608
h 1 -64 -125 3f51bd53
r 2 0 1 42 2 -3 f1ebc4cf19c9723e
h 2 256 -384 3ee6ed78
h 2 383 -257 3e4eb8e2
h 2 261 -307 3eee111e
h 2 320 -381 3ec5666c
r 3 0 1 -17 -1 0 8508d8018f0e1547
h 3 -128 0 3efa5f34
h 3 -1 127 3ed156f6
h 3 -123 77 3f05f103
h 3 -64 3 3e95c34f
r 4 2 1 42 0 -1 ac119e6713311d5e
h 4 0 -128 3e93ecd6
h 4 127 -1 3ea4ac66
h 4 5 -51 3e715cf6
h 4 64 -125 3e76c546
r 5 0 0 -2147483648 1 1 afc4e38f3a9e169a
h 5 128 128 3e671459
h 5 255 255 3e30fdf0
h 5 133 205 3e72d100
h 5 192 131 3e301f6e
";

    fn eroded(
        variant: usize,
        set: usize,
        droplets: i32,
        lifetime: i32,
        smoothing: i32,
    ) -> ErodedNoiseGenerator {
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
        )
    }

    /// FNV-1a over every float's `floatToIntBits` (canonical NaN), in index order.
    fn digest(map: &[f32]) -> u64 {
        map.iter().fold(0xcbf2_9ce4_8422_2325, |hash: u64, value| {
            let bits = if value.is_nan() {
                0x7fc0_0000
            } else {
                value.to_bits()
            };
            (hash ^ u64::from(bits)).wrapping_mul(0x100_0000_01b3)
        })
    }

    /// Whole eroded regions (raw heights, region-wide erosion, smoothing) and cached column heights against
    /// Reimagined's Java, bit for bit, over two levels, both terrain sets, four seeds, negative regions, smoothing on
    /// and off and the jar's droplet lifetime. Each case gets a fresh generator, as the oracle does.
    #[test]
    fn eroded_regions_match_reimagined_java() {
        let mut lines = Vec::new();
        for (case, &(variant, set, seed, region_x, region_z, droplets, lifetime, smoothing)) in
            CASES.iter().enumerate()
        {
            let eroded = eroded(variant, set, droplets, lifetime, smoothing);
            let map = eroded
                .build_region(seed, region_x, region_z)
                .unwrap()
                .heights;
            assert_eq!(map.len(), 256 * 256);
            lines.push(format!(
                "r {case} {variant} {set} {seed} {region_x} {region_z} {:016x}",
                digest(&map)
            ));
            for (dx, dz) in BLOCKS {
                let x = region_x.wrapping_mul(128).wrapping_add(dx);
                let z = region_z.wrapping_mul(128).wrapping_add(dz);
                let height = eroded.eroded_height_noise(seed, x, z).unwrap();
                lines.push(format!("h {case} {x} {z} {:08x}", height.to_bits()));
            }
        }
        assert_eq!(lines, JAVA.lines().collect::<Vec<_>>());
    }

    /// With erosion and smoothing off, the eroded column height is the raw `sample` (terrain, then rivers).
    #[test]
    fn unfiltered_eroded_heights_match_raw_generation() {
        let eroded = eroded(0, 0, 0, 25, 0);
        let seed = -17;
        let terrains = Arc::clone(&eroded.generator().continent().context().terrain);
        for (x, z) in [(0, 0), (15, 15), (16, 31), (-1, -129), (127, 127)] {
            let mut sample = NoiseSample::new(&terrains);
            let mut blender = eroded.generator().blender_resource();
            let expected = eroded
                .generator()
                .sample(seed, x, z, &mut sample, &mut blender)
                .unwrap()
                .height_noise;
            assert_eq!(
                eroded.eroded_height_noise(seed, x, z).unwrap().to_bits(),
                expected.to_bits()
            );
        }
    }

    /// Eight threads reading columns of two regions through one generator get the uncached builds' heights.
    #[test]
    fn cached_heights_equal_uncached_builds_across_threads() {
        let eroded = eroded(0, 0, 50, 25, 1);
        let seed = 42;
        let direct =
            [(0, 0), (-1, 0)].map(|(x, z)| eroded.build_region(seed, x, z).unwrap().heights);
        let columns: Vec<(i32, i32)> = (0..64)
            .map(|i: i32| (i * 37 % 256 - 128, i * 53 % 128))
            .collect();
        thread::scope(|scope| {
            for worker in 0..8 {
                let (eroded, columns, direct) = (&eroded, &columns, &direct);
                scope.spawn(move || {
                    for &(x, z) in columns.iter().cycle().skip(worker * 8).take(64) {
                        let mut sample =
                            NoiseSample::new(&eroded.generator().continent().context().terrain);
                        let mut blender = eroded.generator().blender_resource();
                        let noise_x = eroded.noise_coord(x);
                        let noise_z = eroded.noise_coord(z);
                        eroded
                            .generator()
                            .sample_terrain(seed, noise_x, noise_z, &mut sample, &mut blender)
                            .unwrap();
                        sample.height_noise =
                            direct[usize::from(x < 0)][ErosionRegionLayout::block_index(x, z)];
                        let expected = eroded
                            .generator()
                            .sample_river(seed, noise_x, noise_z, &mut sample)
                            .height_noise;
                        assert_eq!(
                            eroded.eroded_height_noise(seed, x, z).unwrap().to_bits(),
                            expected.to_bits()
                        );
                    }
                });
            }
        });
    }

    /// A custom terrain noise has no seed-first form: the region build fails like Java's throwing `buildRegion`, is
    /// not cached (the next call builds and fails again) and leaves the other regions working.
    #[test]
    fn failed_region_builds_are_errors_and_not_cached() {
        let eroded = ErodedNoiseGenerator::new(
            generator_with(0, |context| {
                vec![Arc::new(TerrainNoise::new(
                    context.terrain.get_by_name("hills").unwrap(),
                    1.0,
                    Module::custom(0.0, 1.0, |_, _| 0.25),
                ))]
            }),
            ErosionFilterSettings::default(),
            HeightmapSmoothingSettings::default(),
        );
        for _ in 0..2 {
            assert!(eroded.build_region(0, 0, 0).is_err());
            assert!(eroded.eroded_height_noise(0, 0, 0).is_err());
            assert!(eroded.region(0, 0, 0).is_err());
            let mut called = false;
            assert!(eroded.generate(0, 3, 5, |_| called = true).is_err());
            assert!(!called);
        }
        let trait_generate = std::panic::catch_unwind(AssertUnwindSafe(|| {
            NoiseGeneratorContract::generate(&eroded, 0, 0, 0, &mut |_| {});
        }));
        assert!(trait_generate.is_err());
    }

    /// `oracles/erosion_region/e2e/chunks.txt` (`run-chunks.sh`): Reimagined's own `generate`, a new `NoiseResource`
    /// per chunk, interpreted and JIT-compiled alike. Groups: levels, terrain set, seed, droplets, lifetime, smoothing
    /// iterations (as in `ErodedChunkOracle.GROUPS`), each with its chunks (`CHUNKS`).
    const CHUNK_GROUPS: [(usize, usize, i32, i32, i32, i32, &[(i32, i32)]); 3] = [
        (
            0,
            1,
            0,
            350,
            25,
            0,
            &[
                (0, 0),
                (7, 7),
                (7, 0),
                (8, 0),
                (-1, -1),
                (-8, -8),
                (-1, -8),
                (24, 16),
                (25, 16),
            ],
        ),
        (
            2,
            0,
            i32::MIN,
            100,
            12,
            1,
            &[(134_217_727, -134_217_728), (-134_217_728, 134_217_727)],
        ),
        (0, 1, i32::MAX, 350, 25, 2, &[(-3, 5)]),
    ];
    const JAVA_CHUNKS: &str = "\
chunk 0 0 0 0 90c473a1155605e7
chunk 0 0 7 7 89d4e2cb8cee19e0
chunk 0 0 7 0 a59de6936e42eb8c
chunk 0 0 8 0 9e51a3f27edec7ce
chunk 0 0 -1 -1 fe440c688d4b931a
chunk 0 0 -8 -8 af629f997f3f5c0d
chunk 0 0 -1 -8 41b3fe90dea5c723
chunk 0 0 24 16 7b26327201704579
chunk 0 0 25 16 c04ac786e1d23b10
chunk 1 -2147483648 134217727 -134217728 4610fb9c442d1a99
chunk 1 -2147483648 -134217728 134217727 96425e42cc85b6a5
chunk 2 2147483647 -3 5 3fba3fdd57ea00f8";

    /// FNV-1a over every element of the chunk's maps in index order (height, base, river, terrain id or -1), then the
    /// data's own sample: `ErodedChunkOracle.fold` (= noise_generator.rs's chunk fold).
    fn fold_chunk(data: &NoiseData) -> u64 {
        let mut fold = Fold::new();
        for index in 0..data.index().length() {
            fold.float(data.height().get_index(index));
            fold.float(data.base().get_index(index));
            fold.float(data.river().get_index(index));
            fold.add(
                data.terrain()
                    .get_index(index)
                    .map_or(-1, |terrain| i64::from(terrain.id())) as u64,
            );
        }
        fold.sample(data.sample());
        fold.0
    }

    fn chunk_digest(eroded: &ErodedNoiseGenerator, seed: i32, chunk_x: i32, chunk_z: i32) -> u64 {
        let mut out = None;
        eroded
            .generate(seed, chunk_x, chunk_z, |data| {
                assert!(out.replace(fold_chunk(data)).is_none());
            })
            .unwrap();
        out.unwrap()
    }

    /// Every element of twelve chunks' `NoiseData` (negative chunks, both sides of region boundaries, region corners,
    /// river chunks, block coordinates wrapping at the `int` limits; seeds 0, MIN and MAX) against Reimagined's Java.
    /// The oracle clears its resource pool before every chunk, so this is the fresh-resource path
    /// ([`RiverStart::Fresh`], `sparse_forests`), not the in-play default.
    #[test]
    fn eroded_chunks_match_reimagined_java() {
        let mut lines = Vec::new();
        for (group, &(variant, set, seed, droplets, lifetime, smoothing, chunks)) in
            CHUNK_GROUPS.iter().enumerate()
        {
            let eroded = eroded(variant, set, droplets, lifetime, smoothing)
                .with_river_start(RiverStart::Fresh);
            for &(chunk_x, chunk_z) in chunks {
                let hash = chunk_digest(&eroded, seed, chunk_x, chunk_z);
                lines.push(format!(
                    "chunk {group} {seed} {chunk_x} {chunk_z} {hash:016x}"
                ));
            }
        }
        assert_eq!(lines, JAVA_CHUNKS.lines().collect::<Vec<_>>());
    }

    /// `erosion_deposits` (Horizon's) changes no field Java fills: with it the chunks still match Reimagined's Java
    /// bit for bit, and the regions' heights are the uncached build's. Its deposits are read from the region's map, so
    /// a chunk's border holds what the neighbouring chunk of the same region holds at that block (no seams at chunk
    /// borders), and without the setting they stay 0.
    #[test]
    fn deposits_change_no_height_and_agree_across_chunks() {
        let mut lines = Vec::new();
        for (group, &(variant, set, seed, droplets, lifetime, smoothing, chunks)) in
            CHUNK_GROUPS.iter().enumerate()
        {
            let eroded = eroded(variant, set, droplets, lifetime, smoothing)
                .with_river_start(RiverStart::Fresh)
                .with_deposits(true);
            for &(chunk_x, chunk_z) in chunks {
                let hash = chunk_digest(&eroded, seed, chunk_x, chunk_z);
                lines.push(format!(
                    "chunk {group} {seed} {chunk_x} {chunk_z} {hash:016x}"
                ));
            }
        }
        assert_eq!(lines, JAVA_CHUNKS.lines().collect::<Vec<_>>());
        let deposits = |eroded: &ErodedNoiseGenerator, chunk_x, chunk_z| {
            let mut out = Vec::new();
            eroded
                .generate(7, chunk_x, chunk_z, |data| {
                    for dz in NoiseData::MIN..NoiseData::MAX {
                        for dx in NoiseData::MIN..NoiseData::MAX {
                            out.push(data.deposit().get(dx, dz));
                        }
                    }
                })
                .unwrap();
            out
        };
        let at = |values: &[f32], dx: i32, dz: i32| values[((dz + 1) * 18 + dx + 1) as usize];
        let on = eroded(0, 1, 50, 12, 0).with_deposits(true);
        let off = eroded(0, 1, 50, 12, 0).with_deposits(false);
        assert!(
            deposits(&off, 2, 3)
                .iter()
                .all(|value| value.to_bits() == 0)
        );
        let (middle, east, south) = (
            deposits(&on, 2, 3),
            deposits(&on, 3, 3),
            deposits(&on, 2, 4),
        );
        assert!(middle.iter().any(|value| *value > 0.0) && middle.iter().any(|value| *value < 0.0));
        for d in NoiseData::MIN..NoiseData::MAX {
            assert_eq!(at(&middle, 16, d).to_bits(), at(&east, 0, d).to_bits());
            assert_eq!(at(&middle, 15, d).to_bits(), at(&east, -1, d).to_bits());
            assert_eq!(at(&middle, d, 16).to_bits(), at(&south, d, 0).to_bits());
        }
        for (dx, dz) in [(0, 0), (5, 9), (15, 15)] {
            let x = 2 * 16 + dx;
            let z = 3 * 16 + dz;
            assert_eq!(
                on.eroded_height(7, x, z).unwrap().to_bits(),
                off.eroded_height(7, x, z).unwrap().to_bits()
            );
        }
    }

    /// A chunk is, position by position, a new sample's raw terrain with the uncached region build's height at the
    /// chunk's offset (the border reading the region's margin), then the rivers carving a river noise that starts at
    /// the generator's [`RiverStart`] (both are checked); the consumer runs once. Its 16x16 interior equals
    /// `eroded_height_noise` (the height Reimagined's `getBaseHeight`, and so the port's `base_height` and
    /// `base_column`, places structures at).
    #[test]
    fn chunks_are_raw_samples_with_eroded_heights() {
        for start in [RiverStart::InPlay, RiverStart::Fresh] {
            raw_samples_with_eroded_heights(start);
        }
    }

    fn raw_samples_with_eroded_heights(start: RiverStart) {
        let eroded = eroded(0, 1, 50, 25, 1).with_river_start(start);
        let seed = 91;
        let terrains = Arc::clone(&eroded.generator().continent().context().terrain);
        // (-1, 0) and (-8, 7) sit on the edges of region (-1, 0): their borders read its margin.
        for (chunk_x, chunk_z) in [(-1, 0), (-8, 7), (-3, 5)] {
            let (region_x, region_z) = ErosionRegionLayout::region(chunk_x, chunk_z);
            let region = eroded
                .build_region(seed, region_x, region_z)
                .unwrap()
                .heights;
            let (offset_x, offset_z) = (
                ErosionRegionLayout::chunk_offset(chunk_x),
                ErosionRegionLayout::chunk_offset(chunk_z),
            );
            let mut blender = eroded.generator().blender_resource();
            let mut calls = 0;
            eroded
                .generate(seed, chunk_x, chunk_z, |data| {
                    calls += 1;
                    for dz in NoiseData::MIN..NoiseData::MAX {
                        for dx in NoiseData::MIN..NoiseData::MAX {
                            let (x, z) = (chunk_x * 16 + dx, chunk_z * 16 + dz);
                            let (noise_x, noise_z) = (eroded.noise_coord(x), eroded.noise_coord(z));
                            let mut sample = NoiseSample::new(&terrains);
                            sample.river_noise = start.value();
                            eroded
                                .generator()
                                .sample_terrain(seed, noise_x, noise_z, &mut sample, &mut blender)
                                .unwrap();
                            sample.height_noise =
                                region[((offset_z + dz) * 256 + offset_x + dx) as usize];
                            eroded
                                .generator()
                                .sample_river(seed, noise_x, noise_z, &mut sample);
                            assert_eq!(
                                data.height().get(dx, dz).to_bits(),
                                sample.height_noise.to_bits()
                            );
                            assert_eq!(
                                data.base().get(dx, dz).to_bits(),
                                sample.base_noise.to_bits()
                            );
                            assert_eq!(
                                data.river().get(dx, dz).to_bits(),
                                sample.river_noise.to_bits()
                            );
                            assert!(
                                data.terrain()
                                    .get(dx, dz)
                                    .unwrap()
                                    .is_same(&sample.terrain_type)
                            );
                            // Inside the chunk the height is `eroded_height_noise`'s.
                            if (0..16).contains(&dx) && (0..16).contains(&dz) {
                                assert_eq!(
                                    data.height().get(dx, dz).to_bits(),
                                    eroded.eroded_height_noise(seed, x, z).unwrap().to_bits()
                                );
                            }
                        }
                    }
                    assert_eq!(
                        fold_sample(data.sample()),
                        fold_sample(&NoiseSample::new(&terrains))
                    );
                })
                .unwrap();
            assert_eq!(calls, 1);
        }
    }

    /// The river noise is the only thing the start changes: by default (the 0 Java's reused samples settle at in
    /// play) every land position carves to 0, and only seaward of the beach does the carver set 1; a fresh start
    /// (`sparse_forests`) keeps the 1 away from the valleys. `new` takes the start from the world's settings.
    #[test]
    fn river_noise_starts_settled_by_default_and_fresh_with_sparse_forests() {
        let settings = |sparse_forests| super::super::terraforged_settings::Settings {
            sparse_forests,
            ..super::super::terraforged_settings::FAITHFUL
        };
        assert_eq!(
            RiverStart::for_settings(settings(false)),
            RiverStart::InPlay
        );
        assert_eq!(RiverStart::for_settings(settings(true)), RiverStart::Fresh);
        for (sparse_forests, start) in [(false, RiverStart::InPlay), (true, RiverStart::Fresh)] {
            super::super::terraforged_settings::set_for_thread(Some(settings(sparse_forests)));
            assert_eq!(eroded(0, 0, 0, 25, 0).river_start(), start);
            for deterministic in [false, true] {
                super::super::terraforged_settings::set_for_thread(Some(
                    super::super::terraforged_settings::Settings {
                        deterministic,
                        ..settings(sparse_forests)
                    },
                ));
                assert_eq!(eroded(0, 0, 0, 25, 0).river_start(), start);
            }
        }
        super::super::terraforged_settings::set_for_thread(None);
        assert_eq!(eroded(0, 0, 0, 25, 0).river_start(), RiverStart::InPlay);

        let (in_play, fresh) = (
            eroded(0, 1, 50, 25, 1),
            eroded(0, 1, 50, 25, 1).with_river_start(RiverStart::Fresh),
        );
        let seed = 0;
        let (mut land, mut sea, mut away_from_rivers) = (0, 0, 0);
        // River chunks of the oracle's first group, and chunks around them.
        for (chunk_x, chunk_z) in [(24, 16), (25, 16), (0, 0), (7, 7), (-1, -1)] {
            let read = |generator: &ErodedNoiseGenerator| {
                let mut out = None;
                generator
                    .generate(seed, chunk_x, chunk_z, |data| {
                        let cells = (NoiseData::MIN..NoiseData::MAX).flat_map(|dz| {
                            (NoiseData::MIN..NoiseData::MAX).map(move |dx| (dx, dz))
                        });
                        out = Some(
                            cells
                                .map(|(dx, dz)| {
                                    (
                                        data.height().get(dx, dz).to_bits(),
                                        data.base().get(dx, dz).to_bits(),
                                        data.terrain().get(dx, dz).unwrap().id(),
                                        data.river().get(dx, dz),
                                    )
                                })
                                .collect::<Vec<_>>(),
                        );
                    })
                    .unwrap();
                out.unwrap()
            };
            for (a, b) in read(&in_play).into_iter().zip(read(&fresh)) {
                assert_eq!((a.0, a.1, a.2), (b.0, b.1, b.2));
                if a.3 == 1.0 {
                    // Seaward of the beach both are reset to 1.
                    assert_eq!(b.3, 1.0);
                    sea += 1;
                } else {
                    assert_eq!(a.3.to_bits(), 0.0f32.to_bits(), "{a:?} vs {b:?}");
                    land += 1;
                    away_from_rivers += usize::from(b.3 > 0.5);
                }
            }
        }
        assert!(
            land > 0 && away_from_rivers > 0,
            "{land} {sea} {away_from_rivers}"
        );
    }

    fn fold_sample(sample: &NoiseSample) -> u64 {
        let mut fold = Fold::new();
        fold.sample(sample);
        fold.0
    }

    /// Eight threads generating chunks of two regions in different orders through one generator get what a fresh
    /// generator gives each chunk alone (cached = uncached; no state carried from chunk to chunk or thread to thread).
    #[test]
    fn chunks_do_not_depend_on_the_thread_or_order() {
        let seed = -17;
        let chunks = [(0, 0), (7, 3), (-1, 0), (-8, 7)];
        let fresh: Vec<u64> = chunks
            .iter()
            .map(|&(x, z)| chunk_digest(&eroded(0, 1, 50, 25, 1), seed, x, z))
            .collect();
        let shared = eroded(0, 1, 50, 25, 1);
        thread::scope(|scope| {
            for worker in 0..8 {
                let (shared, fresh) = (&shared, &fresh);
                scope.spawn(move || {
                    let mut order: Vec<usize> = (0..chunks.len()).collect();
                    order.rotate_left(worker % chunks.len());
                    if worker % 2 == 1 {
                        order.reverse();
                    }
                    for i in order {
                        let (x, z) = chunks[i];
                        assert_eq!(chunk_digest(shared, seed, x, z), fresh[i]);
                    }
                });
            }
        });
    }

    /// Java's `catch (Throwable)`: a consumer that panics is logged, not passed on, and the generator keeps working.
    #[test]
    fn consumer_panics_are_logged_not_raised() {
        let generator = eroded(0, 0, 0, 25, 0);
        assert_eq!(
            generator.generate(7, 0, 0, |_| panic!("consumer failed")),
            Ok(())
        );
        let expected = chunk_digest(&eroded(0, 0, 0, 25, 0), 7, 0, 0);
        assert_eq!(chunk_digest(&generator, 7, 0, 0), expected);
    }

    /// The trait's height noise and levels, and `find`, are the un-eroded generator's (as Reimagined delegates them).
    #[test]
    fn height_levels_and_find_stay_on_the_raw_generator() {
        let eroded = eroded(0, 1, 350, 25, 0);
        let raw = eroded.generator();
        let seed = 31;
        for (x, z) in [(80, -32), (-1, 0), (1000, 77)] {
            assert_eq!(
                NoiseGeneratorContract::height_noise(&eroded, seed, x, z).to_bits(),
                NoiseGeneratorContract::height_noise(raw, seed, x, z).to_bits()
            );
        }
        assert_eq!(
            NoiseGeneratorContract::levels(&eroded),
            NoiseGeneratorContract::levels(raw)
        );
        assert_eq!(
            NoiseGeneratorContract::terrain_levels(&eroded),
            raw.levels()
        );
        let terrain = raw
            .continent()
            .context()
            .terrain
            .get_by_name("hills")
            .unwrap();
        assert_eq!(
            eroded.find(seed, 0, 0, 1, 4, &terrain),
            raw.find(seed, 0, 0, 1, 4, &terrain)
        );
    }
}
