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
// `buildRegion`, `erodedHeightNoise`, `erodedHeight`, `getRegion`, `generate`, `find`, `getHeightNoise`, `getLevels` and
// `getTerrainLevels` of
// `src/main/java/com/terraforged/mod/worldgen/noise/erosion/ErodedNoiseGenerator.java`. MIT License, Copyright (c) 2020
// TerraForged, Copyright (c) 2026 Romoslayer; see `LICENSE-TERRAFORGED-REIMAGINED`.

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

/// TerraForged Reimagined 26.3's `ErodedNoiseGenerator` (Romoslayer/TerraForged-Reimagined a24d450): erosion runs once per [`ErosionRegionLayout`] region (8x8 chunks plus 4
/// chunks of margin, a 256x256 map) over the raw terrain heights of the wrapped [`NoiseGenerator`], then the optional
/// [`HeightmapSmoothing`], and the result is cached per region and seed ([`ErosionRegionCache`], Java's `getRegion`).
/// Neither the 1.18.2-0.3.1 jar nor the newer source (6dd607eb) has regions: they erode a `NoiseTileSize` tile per
/// chunk. Reimagined's `NoiseGenerator.sampleTerrain`/`sampleRiver` are 6dd607eb's, which [`NoiseGenerator`] ports.
/// Chunks are filled from the regions ([`Self::generate`]); the plain height and `find` stay on the un-eroded generator.
pub struct ErodedNoiseGenerator {
    erosion: ErosionFilter,
    generator: NoiseGenerator,
    smoothing: HeightmapSmoothingSettings,
    regions: ErosionRegionCache<Vec<f32>>,
}

impl ErodedNoiseGenerator {
    /// `new ErodedNoiseGenerator(generator, filters)`: `filters.erosion`'s six values become the region-wide
    /// [`ErosionFilter`]'s settings on a [`ErosionRegionLayout::MAP_SIZE`] map; `filters.smoothing` is kept as is.
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
        }
    }

    /// The wrapped, un-eroded generator (Java's `generator` field).
    pub fn generator(&self) -> &NoiseGenerator {
        &self.generator
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
    fn region(&self, seed: i32, chunk_x: i32, chunk_z: i32) -> Result<Arc<Vec<f32>>, &'static str> {
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
    /// over from the same position of whichever chunk used that resource before. The port resets the sample at every
    /// position (Java's new resource), so a chunk's data never depends on the chunks generated before it or on which
    /// thread generated them; it also fills a new `NoiseData` per call, whose own `sample` stays the defaults (Java
    /// never writes it here).
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
    /// on the first failing sample.
    fn build_region(
        &self,
        seed: i32,
        region_x: i32,
        region_z: i32,
    ) -> Result<Vec<f32>, &'static str> {
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
        Ok(map)
    }
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
            let map = eroded.build_region(seed, region_x, region_z).unwrap();
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
        let direct = [(0, 0), (-1, 0)].map(|(x, z)| eroded.build_region(seed, x, z).unwrap());
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
    #[test]
    fn eroded_chunks_match_reimagined_java() {
        let mut lines = Vec::new();
        for (group, &(variant, set, seed, droplets, lifetime, smoothing, chunks)) in
            CHUNK_GROUPS.iter().enumerate()
        {
            let eroded = eroded(variant, set, droplets, lifetime, smoothing);
            for &(chunk_x, chunk_z) in chunks {
                let hash = chunk_digest(&eroded, seed, chunk_x, chunk_z);
                lines.push(format!(
                    "chunk {group} {seed} {chunk_x} {chunk_z} {hash:016x}"
                ));
            }
        }
        assert_eq!(lines, JAVA_CHUNKS.lines().collect::<Vec<_>>());
    }

    /// A chunk is, position by position, a new sample's raw terrain with the uncached region build's height at the
    /// chunk's offset (the border reading the region's margin), then the rivers; the consumer runs once. Its 16x16
    /// interior equals `eroded_height_noise`, the height structures are placed at.
    #[test]
    fn chunks_are_raw_samples_with_eroded_heights() {
        let eroded = eroded(0, 1, 50, 25, 1);
        let seed = 91;
        let terrains = Arc::clone(&eroded.generator().continent().context().terrain);
        // (-1, 0) and (-8, 7) sit on the edges of region (-1, 0): their borders read its margin.
        for (chunk_x, chunk_z) in [(-1, 0), (-8, 7), (-3, 5)] {
            let (region_x, region_z) = ErosionRegionLayout::region(chunk_x, chunk_z);
            let region = eroded.build_region(seed, region_x, region_z).unwrap();
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
                            // Inside the chunk the height is the one structures are placed at.
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
