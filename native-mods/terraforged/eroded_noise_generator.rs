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
// `buildRegion`, `erodedHeightNoise`, `erodedHeight` and `getRegion` of
// `src/main/java/com/terraforged/mod/worldgen/noise/erosion/ErodedNoiseGenerator.java`. MIT License, Copyright (c) 2020
// TerraForged, Copyright (c) 2026 Romoslayer; see `LICENSE-TERRAFORGED-REIMAGINED`.

use super::{
    terraforged_erosion::{
        ErosionFilter, ErosionFilterSettings, ErosionRegionCache, ErosionRegionLayout,
        ErosionResource, HeightmapSmoothing, HeightmapSmoothingSettings,
    },
    terraforged_noise_generator::NoiseGenerator,
    terraforged_terrain_data::NoiseSample,
};
use std::sync::Arc;

/// TerraForged Reimagined 26.3's `ErodedNoiseGenerator` (Romoslayer/TerraForged-Reimagined a24d450), so far its
/// region build and the eroded column height: erosion runs once per [`ErosionRegionLayout`] region (8x8 chunks plus 4
/// chunks of margin, a 256x256 map) over the raw terrain heights of the wrapped [`NoiseGenerator`], then the optional
/// [`HeightmapSmoothing`], and the result is cached per region and seed ([`ErosionRegionCache`], Java's `getRegion`).
/// Neither the 1.18.2-0.3.1 jar nor the newer source (6dd607eb) has regions: they erode a `NoiseTileSize` tile per
/// chunk. Reimagined's `NoiseGenerator.sampleTerrain`/`sampleRiver` are 6dd607eb's, which [`NoiseGenerator`] ports.
/// Chunk generation from the regions (`generate`) is not ported yet.
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

#[cfg(test)]
mod tests {
    use super::super::{
        terraforged::Module,
        terraforged_noise_generator::tests::{generator, generator_with},
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
        }
    }
}
