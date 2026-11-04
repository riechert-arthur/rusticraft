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
    terraforged_concurrent::LazyCallable,
    terraforged_engine::{Cell, TerrainRegistry},
    terraforged_filters::WorldFilters,
    terraforged_river::Rivermap,
    terraforged_tile::{BatchChunk, Tile},
};
use std::sync::Arc;

/// The stages of Engine's `Heightmap` that `Tile.generate(Heightmap ...)` calls, in the order the
/// tile fills run them: `applyBase`, `getContinent().getRivermap(x, z)` (when the cell's continent
/// cell has no river map yet), `applyRivers` and `applyClimate`. `Heightmap` itself is not ported
/// yet, so the generator takes any implementation.
pub trait HeightmapSource {
    /// `applyBase(cell, x, z)`.
    fn apply_base(&self, cell: &mut Cell, x: f32, z: f32);

    /// `getContinent().getRivermap(x, z)`: the river map of a continent cell.
    fn rivermap(&self, x: i32, z: i32) -> Arc<Rivermap>;

    /// `applyRivers(cell, x, z, rivermap)`.
    fn apply_rivers(&self, cell: &mut Cell, x: f32, z: f32, rivermap: &Rivermap);

    /// `applyClimate(cell, x, z)`.
    fn apply_climate(&self, cell: &mut Cell, x: f32, z: f32);
}

/// What `postProcess` runs on a filled tile: Engine's `WorldFilters.apply(tile, optionalFilters)`.
pub trait TileFilters {
    fn apply(&self, tile: &mut Tile, optional: bool);
}

impl TileFilters for WorldFilters {
    fn apply(&self, tile: &mut Tile, optional: bool) {
        self.apply_tile(tile, optional);
    }
}

/// Engine's `WorldGenerator`: the world's heightmap and filters, which `WorldGeneratorFactory`
/// builds from the generator context.
pub struct WorldGenerator<H, F> {
    heightmap: H,
    filters: F,
}

impl<H, F> WorldGenerator<H, F> {
    pub const fn new(heightmap: H, filters: F) -> Self {
        Self { heightmap, filters }
    }

    /// `getHeightmap()`.
    pub const fn heightmap(&self) -> &H {
        &self.heightmap
    }

    /// `getFilters()`.
    pub const fn filters(&self) -> &F {
        &self.filters
    }
}

/// Engine 0.3.0's plain, synchronous `TileGenerator` (`com.terraforged.engine.tile.gen`, what
/// `TileGenerator.builder().build()` returns without a batch size): the size factor and border
/// (in chunks) of its tiles, the batch size (kept for the batched generators), the world's
/// registry the tiles' cells start from (in place of Engine's `TileResources` pools) and the
/// world generator. Engine's `TileGeneratorBatched` and `TileGeneratorStriped` are the
/// `generate_batched_*` and `generate_striped_*` methods. The async wrapper, the tile cache, the
/// builder and tile disposal are not ported.
pub struct TileGenerator<H, F> {
    factor: i32,
    border: i32,
    batch_size: i32,
    terrains: Arc<TerrainRegistry>,
    generator: WorldGenerator<H, F>,
}

impl<H, F> TileGenerator<H, F>
where
    H: HeightmapSource,
    F: TileFilters,
{
    /// `TileGenerator.builder().size(factor, border).batch(batch_size)` with a factory supplying
    /// `generator`; `terrains` is the world's registry.
    pub const fn new(
        factor: i32,
        border: i32,
        batch_size: i32,
        terrains: Arc<TerrainRegistry>,
        generator: WorldGenerator<H, F>,
    ) -> Self {
        Self {
            factor,
            border,
            batch_size,
            terrains,
            generator,
        }
    }

    /// `getGenerator()`.
    pub const fn generator(&self) -> &WorldGenerator<H, F> {
        &self.generator
    }

    /// The builder's `batchSize`, which the plain generator ignores.
    pub const fn batch_size(&self) -> i32 {
        self.batch_size
    }

    /// `chunkToRegion(i)`: `i >> factor`, the shift count taken modulo 32 as in Java.
    pub const fn chunk_to_region(&self, chunk: i32) -> i32 {
        chunk.wrapping_shr(self.factor as u32)
    }

    /// `getTile(regionX, regionZ)`: a `CallableTile`, which runs [`Self::generate_region`] on its
    /// first `get` (nothing runs before it) and hands every later caller that tile. Like
    /// `CallableTile`, which keeps a reference to its generator, the handle borrows `self`; it is
    /// `Send + Sync` when the generator is, so threads can share it.
    pub fn get_tile(
        &self,
        region_x: i32,
        region_z: i32,
    ) -> LazyCallable<Tile, impl Fn() -> Tile + '_> {
        LazyCallable::new(move || self.generate_region(region_x, region_z))
    }

    /// `getTile(centerX, centerZ, zoom, filter)`: a `CallableZoomTile`, which runs
    /// [`Self::generate_zoomed_region`] on its first `get` and hands every later caller that tile.
    pub fn get_zoomed_tile(
        &self,
        center_x: f32,
        center_z: f32,
        zoom: f32,
        filter: bool,
    ) -> LazyCallable<Tile, impl Fn() -> Tile + '_> {
        LazyCallable::new(move || self.generate_zoomed_region(center_x, center_z, zoom, filter))
    }

    /// `generateRegion(regionX, regionZ)`: an empty region ([`Self::create_empty_region`]), filled
    /// whole by [`Tile::generate_heightmap`] with the heightmap's stages, then `postProcess(tile)`:
    /// the filters with the optional ones (`apply(tile, true)`), which see every cell of the tile,
    /// border included, only after the fill.
    pub fn generate_region(&self, region_x: i32, region_z: i32) -> Tile {
        let mut tile = self.create_empty_region(region_x, region_z);
        let heightmap = self.generator.heightmap();
        tile.generate_heightmap(
            |cell, x, z| heightmap.apply_base(cell, x, z),
            |x, z| heightmap.rivermap(x, z),
            |cell, x, z, map| heightmap.apply_rivers(cell, x, z, map),
            |cell, x, z| heightmap.apply_climate(cell, x, z),
        );
        self.generator.filters().apply(&mut tile, true);
        tile
    }

    /// `generateRegion(centerX, centerZ, zoom, filter)`: region (0, 0), filled by
    /// [`Tile::generate_zoomed_heightmap`] with the center as its offset (uncached, as Engine's
    /// `tile.generate(heightmap, centerX, centerZ, zoom)`), then `postProcess(tile, filter)`: the
    /// filters, the optional ones only when `filter` is set. The filters are seeded with region
    /// (0, 0) whatever the center.
    pub fn generate_zoomed_region(
        &self,
        center_x: f32,
        center_z: f32,
        zoom: f32,
        filter: bool,
    ) -> Tile {
        let mut tile = self.create_empty_region(0, 0);
        let heightmap = self.generator.heightmap();
        tile.generate_zoomed_heightmap(
            center_x,
            center_z,
            zoom,
            |cell, x, z| heightmap.apply_base(cell, x, z),
            |x, z| heightmap.rivermap(x, z),
            |cell, x, z, map| heightmap.apply_rivers(cell, x, z, map),
            |cell, x, z| heightmap.apply_climate(cell, x, z),
        );
        self.generator.filters().apply(&mut tile, filter);
        tile
    }

    /// `TileGeneratorBatched.generateRegion(regionX, regionZ)` with `batch(batch_count)`: an empty
    /// region filled by [`Tile::generate_area`] (`batch_count` squared batches of chunks on
    /// [`batch_workers`] threads, each chunk as [`drive_chunk`]), then the filters with the optional
    /// ones. The cells equal [`Self::generate_region`]'s for any batch count; only the river-map
    /// lookups differ, as each chunk starts without one. Engine's builder never makes a batched
    /// generator for a count below 1; called with one, it throws, and so does this:
    /// `ArithmeticException` for 0 (`getBatchSize`), and for a negative count, which fills no cell,
    /// a `NullPointerException` once the filters read the cells it never created. A stage's panic
    /// is re-raised.
    pub fn generate_batched_region(&self, region_x: i32, region_z: i32, batch_count: i32) -> Tile
    where
        H: Sync,
    {
        check_batch_count(batch_count);
        let heightmap = self.generator.heightmap();
        let mut tile = self.create_empty_region(region_x, region_z);
        tile.generate_area(batch_count, batch_workers(), |chunk| {
            drive_chunk(chunk, heightmap, None);
        });
        self.generator.filters().apply(&mut tile, true);
        tile
    }

    /// `TileGeneratorBatched.generateRegion(centerX, centerZ, zoom, filter)`: region (0, 0) filled
    /// by [`Tile::generate_area`] at [`Tile::generate_zoomed_heightmap`]'s positions, then the
    /// filters, the optional ones only when `filter` is set. Panics as
    /// [`Self::generate_batched_region`].
    pub fn generate_batched_zoomed_region(
        &self,
        center_x: f32,
        center_z: f32,
        zoom: f32,
        filter: bool,
        batch_count: i32,
    ) -> Tile
    where
        H: Sync,
    {
        check_batch_count(batch_count);
        let heightmap = self.generator.heightmap();
        let mut tile = self.create_empty_region(0, 0);
        let zoom = Some(Zoom::new(&tile, center_x, center_z, zoom));
        tile.generate_area(batch_count, batch_workers(), |chunk| {
            drive_chunk(chunk, heightmap, zoom);
        });
        self.generator.filters().apply(&mut tile, filter);
        tile
    }

    /// `TileGeneratorStriped.generateRegion(regionX, regionZ)` with `batch(sections)`: as
    /// [`Self::generate_batched_region`], each chunk row cut into `sections` stripes
    /// ([`Tile::generate_area_striped`]).
    pub fn generate_striped_region(&self, region_x: i32, region_z: i32, sections: i32) -> Tile
    where
        H: Sync,
    {
        check_batch_count(sections);
        let heightmap = self.generator.heightmap();
        let mut tile = self.create_empty_region(region_x, region_z);
        tile.generate_area_striped(sections, batch_workers(), |chunk| {
            drive_chunk(chunk, heightmap, None);
        });
        self.generator.filters().apply(&mut tile, true);
        tile
    }

    /// `TileGeneratorStriped.generateRegion(centerX, centerZ, zoom, filter)`: as
    /// [`Self::generate_batched_zoomed_region`], in stripes.
    pub fn generate_striped_zoomed_region(
        &self,
        center_x: f32,
        center_z: f32,
        zoom: f32,
        filter: bool,
        sections: i32,
    ) -> Tile
    where
        H: Sync,
    {
        check_batch_count(sections);
        let heightmap = self.generator.heightmap();
        let mut tile = self.create_empty_region(0, 0);
        let zoom = Some(Zoom::new(&tile, center_x, center_z, zoom));
        tile.generate_area_striped(sections, batch_workers(), |chunk| {
            drive_chunk(chunk, heightmap, zoom);
        });
        self.generator.filters().apply(&mut tile, filter);
        tile
    }

    /// `createEmptyRegion(regionX, regionZ)`: a fresh tile of the generator's size factor and
    /// border, its cells from the world's registry.
    pub fn create_empty_region(&self, region_x: i32, region_z: i32) -> Tile {
        Tile::new(region_x, region_z, self.factor, self.border, &self.terrains)
    }
}

/// Panics for a negative batch count, where Engine's batched generators throw (see
/// [`TileGenerator::generate_batched_region`]); a zero count panics in [`Tile::batch_size`].
fn check_batch_count(count: i32) {
    assert!(
        count >= 0,
        "batch count {count}: Engine fills no cell and its filters throw NullPointerException"
    );
}

/// The batch threads: Engine's default pool (`ThreadPools.createDefault()`) has `max(2,
/// processors)` batch workers.
fn batch_workers() -> usize {
    std::thread::available_parallelism().map_or(2, |n| n.get().max(2))
}

/// The zoomed tasks' `translateX`/`translateZ` and zoom, as `generateArea(.., offsetX, offsetZ,
/// zoom)` computes them: `offset - (float) blockSize.size * zoom / 2`, in `float` math.
#[derive(Clone, Copy)]
struct Zoom {
    translate_x: f32,
    translate_z: f32,
    zoom: f32,
}

impl Zoom {
    fn new(tile: &Tile, offset_x: f32, offset_z: f32, zoom: f32) -> Self {
        let size = tile.block_size().size as f32;
        Self {
            translate_x: offset_x - size * zoom / 2.0,
            translate_z: offset_z - size * zoom / 2.0,
            zoom,
        }
    }
}

/// `ChunkBatchTask.driveOne` (and the striped and zoomed tasks', which are the same): each cell,
/// row by row, at its world block position (zoomed: `(float) block * zoom + translate`) through
/// `applyBase`, the river map (`Rivermap.get`, starting from none in every chunk), `applyRivers`
/// and `applyClimate`.
fn drive_chunk<H: HeightmapSource>(chunk: &mut BatchChunk, heightmap: &H, zoom: Option<Zoom>) {
    let mut current = None;
    chunk.generate(|cell, _, _, block_x, block_z| {
        let (x, z) = match zoom {
            None => (block_x as f32, block_z as f32),
            Some(t) => (
                block_x as f32 * t.zoom + t.translate_x,
                block_z as f32 * t.zoom + t.translate_z,
            ),
        };
        heightmap.apply_base(cell, x, z);
        let rivermap =
            Rivermap::get_for_cell(cell, current.take(), |x, z| heightmap.rivermap(x, z));
        heightmap.apply_rivers(cell, x, z, &rivermap);
        heightmap.apply_climate(cell, x, z);
        current = Some(rivermap);
    });
}

#[cfg(test)]
mod tests {
    use super::super::{
        terraforged_engine::{Levels, Terrain, WorldControlPoints},
        terraforged_filters::FilterSettings,
        terraforged_river::GenWarp,
    };
    use super::*;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicI32, AtomicI64, Ordering};

    fn fold(fold: i64, value: i32) -> i64 {
        fold.wrapping_mul(31).wrapping_add(i64::from(value))
    }

    fn hash(x: i32, z: i32) -> u32 {
        let mut h = x.wrapping_mul(7919).wrapping_add(z.wrapping_mul(104_729));
        h ^= ((h as u32) >> 13) as i32;
        h = h.wrapping_mul(0x5bd1_e995);
        (h ^ ((h as u32) >> 15) as i32) as u32
    }

    /// `TileGeneratorOracle.java`'s `FillHeightmap`: stages that hash heights, continent edges,
    /// terrains and erosion and river masks from the block position, and fold every call: into
    /// one trace in call order (meaningful for single-threaded fills) and, each call folded on its
    /// own, into a sum that does not depend on the order threads run them in (the oracle's `call`).
    struct FillHeightmap {
        kinds: [Terrain; 4],
        trace: Mutex<i64>,
        sum: AtomicI64,
        lookups: AtomicI32,
    }

    impl FillHeightmap {
        fn new(kinds: [Terrain; 4]) -> Self {
            Self {
                kinds,
                trace: Mutex::new(17),
                sum: AtomicI64::new(0),
                lookups: AtomicI32::new(0),
            }
        }

        fn call(&self, values: &[i32]) {
            let mut trace = self.trace.lock().unwrap();
            *trace = values.iter().fold(*trace, |f, &v| fold(f, v));
            let own = values.iter().fold(17, |f, &v| fold(f, v));
            self.sum.fetch_add(own, Ordering::SeqCst);
        }

        fn call_point(&self, stage: i32, x: f32, z: f32) {
            self.call(&[stage, x.to_bits() as i32, z.to_bits() as i32]);
        }

        /// The stage trace and lookup count since the last call.
        fn take(&self) -> (i32, i64) {
            self.sum.store(0, Ordering::SeqCst);
            let trace = std::mem::replace(&mut *self.trace.lock().unwrap(), 17);
            (self.lookups.swap(0, Ordering::SeqCst), trace)
        }

        /// The lookup count and the order-free stage sum since the last call.
        fn take_sum(&self) -> (i32, i64) {
            *self.trace.lock().unwrap() = 17;
            let sum = self.sum.swap(0, Ordering::SeqCst);
            (self.lookups.swap(0, Ordering::SeqCst), sum)
        }
    }

    impl HeightmapSource for FillHeightmap {
        fn apply_base(&self, cell: &mut Cell, x: f32, z: f32) {
            self.call_point(1, x, z);
            let (x, z) = (x as i32, z as i32);
            let h = hash(x, z);
            cell.value = 3_i32
                .wrapping_mul(x)
                .wrapping_add(2_i32.wrapping_mul(z))
                .rem_euclid(97) as f32
                / 2000.0
                + 0.2
                + ((h >> 8) & 0xFFFF) as f32 / 40_000_000.0;
            cell.continent_edge = ((h >> 12) & 0x3FF) as f32 / 1600.0;
            cell.terrain = self.kinds[((h >> 1) & 3) as usize].clone();
            cell.erosion_mask = h & 63 == 17;
            cell.continent_x = x.div_euclid(24);
            cell.continent_z = z.div_euclid(40);
        }

        fn rivermap(&self, x: i32, z: i32) -> Arc<Rivermap> {
            self.lookups.fetch_add(1, Ordering::SeqCst);
            self.call(&[1000, x, z]);
            Arc::new(Rivermap::new(x, z, Vec::new(), GenWarp::direct()))
        }

        fn apply_rivers(&self, cell: &mut Cell, x: f32, z: f32, map: &Rivermap) {
            let (bx, bz) = (x.to_bits() as i32, z.to_bits() as i32);
            self.call(&[2, bx, bz, map.x(), map.z()]);
            let h = hash(x as i32, z as i32);
            let mask = if (h >> 4) & 3 == 0 { 0.05 } else { 1.0 };
            cell.river_mask = mask - (map.x() + map.z()).rem_euclid(3) as f32 * 0.01;
        }

        fn apply_climate(&self, cell: &mut Cell, x: f32, z: f32) {
            self.call_point(3, x, z);
            cell.temperature = z * 0.001 - cell.river_mask;
            cell.moisture += x * 0.001;
        }
    }

    /// Every backing cell's fields, NaNs folded as Java's `floatToIntBits`.
    fn fold_tile(tile: &Tile) -> i64 {
        let bits = |value: f32| {
            if value.is_nan() {
                0x7fc0_0000
            } else {
                value.to_bits() as i32
            }
        };
        let mut f = 17_i64;
        let total = tile.block_size().total;
        let cells = (0..total).flat_map(|z| (0..total).map(move |x| (x, z)));
        for cell in cells.map(|(x, z)| tile.raw_cell(x, z).expect("backing cell")) {
            for value in [
                cell.value,
                cell.erosion,
                cell.sediment,
                cell.gradient,
                cell.moisture,
                cell.temperature,
                cell.continent_edge,
                cell.river_mask,
            ] {
                f = fold(f, bits(value));
            }
            for value in [
                cell.continent_x,
                cell.continent_z,
                i32::from(cell.erosion_mask),
                cell.terrain.id(),
            ] {
                f = fold(f, value);
            }
        }
        f
    }

    /// Replays `TileGeneratorOracle.java` (`.local/terraforged/oracles/tile_generator/`): Engine
    /// 0.3.0's plain `TileGenerator` built with `size(factor, border)` and a
    /// `WorldGeneratorFactory(ctx, heightmap)`, over the stub heightmap, for two filter settings
    /// groups and six sizes and regions (borderless, wide-border, negative and wrapping ones).
    /// `g` lines are `generateRegion(regionX, regionZ)` (fill, then the filters with the optional
    /// ones); `z` lines the zoomed `generateRegion(centerX, centerZ, zoom, filter)` on the same
    /// generator (fractional, unit, negative, NaN and huge centers and zooms, filters on and off);
    /// `l` and `m` lines the lazy `getTile(regionX, regionZ)` and `getTile(centerX, centerZ, zoom,
    /// filter)` handles (nothing runs before the first `get`, a second `get` returns the same tile
    /// without running anything); `b` and `y` lines Engine's `TileGeneratorBatched` and
    /// `TileGeneratorStriped` (`batch(n)`, on a four-thread pool) regular and zoomed for batch
    /// counts 1, 3, 2, 7 and 100, with the lookup count and an order-free sum of the stage calls
    /// (their cells equal the `g`/`z` lines'); `e` lines counts 0 and -2, where Engine throws; `s`
    /// lines `chunkToRegion` for shift factors Java masks to five bits.
    #[test]
    fn tile_generator_matches_engine_oracle() {
        let terrains = Arc::new(TerrainRegistry::new());
        let kinds = ["none", "coast", "beach", "hills"]
            .map(|name| terrains.get_by_name(name).expect("built-in terrain"));
        let groups = [(12345, 20, 1, 0.327), (-9, 5, 2, 0.4)];
        let zooms = [
            (100.0, -50.0, 0.5, false),
            (-37.25, 12.5, 2.0, true),
            (0.0, 0.0, 1.0, true),
            (f32::NAN, 3.5, 0.75, true),
            (1e9, -1e9, -3.0, false),
        ];
        let mut out = String::new();
        for (run, (group, factor, border, region_x, region_z)) in [
            (0, 1, 1, -1, 2),
            (0, 2, 1, 3, -5),
            (1, 1, 0, 0, 0),
            (1, 0, 2, -7, 9),
            (0, 3, 2, 1, -1),
            (1, 1, 1, 0x1000_0000, -0x1000_0001),
        ]
        .into_iter()
        .enumerate()
        {
            let (seed, droplets, passes, beach) = groups[group];
            let mut settings = FilterSettings::default();
            settings.erosion.droplets_per_chunk = droplets;
            settings.smoothing.iterations = passes;
            let points = WorldControlPoints {
                beach,
                ..WorldControlPoints::default()
            };
            let filters =
                WorldFilters::new(seed, settings, Levels::new(256, 63), points, &terrains);
            let heightmap = FillHeightmap::new(kinds.clone());
            let generator = TileGenerator::new(
                factor,
                border,
                0,
                Arc::clone(&terrains),
                WorldGenerator::new(heightmap, filters),
            );
            let heightmap = generator.generator().heightmap();
            let tile = generator.generate_region(region_x, region_z);
            let (lookups, trace) = heightmap.take();
            let cells = fold_tile(&tile);
            out += &format!("g {run} {lookups} {trace:x} {cells:x}\n");
            for (pair, (center_x, center_z, zoom, filter)) in zooms.into_iter().enumerate() {
                let tile = generator.generate_zoomed_region(center_x, center_z, zoom, filter);
                let (lookups, trace) = heightmap.take();
                let (rx, rz, cells) = (tile.region_x(), tile.region_z(), fold_tile(&tile));
                out += &format!("z {run} {pair} {rx} {rz} {lookups} {trace:x} {cells:x}\n");
            }
            for striped in [false, true] {
                for count in [1, 3, 2, 7, 100] {
                    let tile = if striped {
                        generator.generate_striped_region(region_x, region_z, count)
                    } else {
                        generator.generate_batched_region(region_x, region_z, count)
                    };
                    let (lookups, sum) = heightmap.take_sum();
                    let (st, cells) = (u8::from(striped), fold_tile(&tile));
                    out += &format!("b {run} {st} {count} {lookups} {sum:x} {cells:x}\n");
                    let set = (run + count as usize) % zooms.len();
                    let (center_x, center_z, zoom, filter) = zooms[set];
                    let tile = if striped {
                        generator
                            .generate_striped_zoomed_region(center_x, center_z, zoom, filter, count)
                    } else {
                        generator
                            .generate_batched_zoomed_region(center_x, center_z, zoom, filter, count)
                    };
                    let (lookups, sum) = heightmap.take_sum();
                    let cells = fold_tile(&tile);
                    out += &format!("y {run} {st} {count} {set} {lookups} {sum:x} {cells:x}\n");
                }
            }
            for striped in [false, true] {
                for count in [0, -2] {
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        if striped {
                            generator.generate_striped_region(region_x, region_z, count)
                        } else {
                            generator.generate_batched_region(region_x, region_z, count)
                        }
                    }));
                    assert!(result.is_err());
                    let thrown = if count == 0 {
                        "ArithmeticException"
                    } else {
                        "NullPointerException"
                    };
                    out += &format!("e {run} {} {count} throws {thrown}\n", u8::from(striped));
                }
            }
            // A negative count on the tile itself runs no stage (Engine's synchronous batcher).
            let mut tile = generator.create_empty_region(region_x, region_z);
            tile.generate_area(-2, 4, |chunk| drive_chunk(chunk, heightmap, None));
            tile.generate_area_striped(-2, 4, |chunk| drive_chunk(chunk, heightmap, None));
            assert_eq!(heightmap.take_sum(), (0, 0));
            let (center_x, center_z, zoom, filter) = zooms[run % zooms.len()];
            let regular = generator.get_tile(region_x, region_z);
            let zoomed = generator.get_zoomed_tile(center_x, center_z, zoom, filter);
            for (kind, lazy) in [("l", &regular as &dyn LazyTile), ("m", &zoomed)] {
                let done = lazy.done();
                let (lookups, trace) = heightmap.take();
                let first = lazy.tile();
                let (first_lookups, first_trace) = heightmap.take();
                let again = lazy.tile();
                let (again_lookups, again_trace) = heightmap.take();
                let (rx, rz) = (first.region_x(), first.region_z());
                let cells = fold_tile(first);
                let same = std::ptr::eq(first, again);
                out += &format!(
                    "{kind} {run} {done} {lookups} {trace:x} {first_lookups} {first_trace:x} {rx} {rz} \
                     {cells:x} {same} {again_lookups} {again_trace:x} {}\n",
                    lazy.done()
                );
            }
        }
        let chunks = [0, 1, -1, 7, -8, 1000, -1000, i32::MAX, i32::MIN];
        for factor in [0, 1, 3, 31, 32, 33, -1, -31] {
            let generator = TileGenerator::new(
                factor,
                0,
                0,
                Arc::clone(&terrains),
                WorldGenerator::new(FillHeightmap::new(kinds.clone()), NoFilters),
            );
            out += &format!("s {factor}");
            for chunk in chunks {
                out += &format!(" {}", generator.chunk_to_region(chunk));
            }
            out += "\n";
        }
        assert_eq!(out, ORACLE);
    }

    /// The two lazy handle types behind one interface, for the oracle loop.
    trait LazyTile {
        fn done(&self) -> bool;
        fn tile(&self) -> &Tile;
    }

    impl<F: Fn() -> Tile> LazyTile for LazyCallable<Tile, F> {
        fn done(&self) -> bool {
            self.is_done()
        }

        fn tile(&self) -> &Tile {
            self.get()
        }
    }

    struct NoFilters;

    impl TileFilters for NoFilters {
        fn apply(&self, _: &mut Tile, _: bool) {}
    }

    /// Batches give the same cells on one thread or many (each chunk's cells belong to one batch),
    /// odd and uneven counts included, and a stage's panic reaches the caller with its payload.
    #[test]
    fn batches_match_any_thread_count_and_propagate_panics() {
        let terrains = Arc::new(TerrainRegistry::new());
        let kinds = ["none", "coast", "beach", "hills"]
            .map(|name| terrains.get_by_name(name).expect("built-in terrain"));
        let heightmap = FillHeightmap::new(kinds);
        for (count, workers) in [(1, 8), (3, 1), (3, 8), (5, 3), (2, 64)] {
            let fill = |workers, striped| {
                let mut tile = Tile::new(-3, 4, 2, 1, &terrains);
                let drive = |chunk: &mut BatchChunk| drive_chunk(chunk, &heightmap, None);
                if striped {
                    tile.generate_area_striped(count, workers, drive);
                } else {
                    tile.generate_area(count, workers, drive);
                }
                (fold_tile(&tile), heightmap.take_sum())
            };
            for striped in [false, true] {
                assert!(fill(workers, striped) == fill(1, striped));
            }
        }
        let panicked = std::panic::catch_unwind(|| {
            let mut tile = Tile::new(0, 0, 2, 0, &terrains);
            tile.generate_area(2, 4, |chunk| {
                if chunk.chunk().chunk_x() == 3 {
                    std::panic::panic_any(chunk.chunk().chunk_z());
                }
            });
        });
        let payload = panicked.expect_err("a batch panicked");
        assert!(matches!(payload.downcast_ref::<i32>(), Some(0..=3)));
    }

    /// Engine shares one tile generator between its generation threads.
    #[test]
    fn tile_generators_are_shared_between_threads() {
        fn shareable<T: Send + Sync>() {}
        struct SharedHeightmap;
        impl HeightmapSource for SharedHeightmap {
            fn apply_base(&self, _: &mut Cell, _: f32, _: f32) {}
            fn rivermap(&self, x: i32, z: i32) -> Arc<Rivermap> {
                Arc::new(Rivermap::new(x, z, Vec::new(), GenWarp::direct()))
            }
            fn apply_rivers(&self, _: &mut Cell, _: f32, _: f32, _: &Rivermap) {}
            fn apply_climate(&self, _: &mut Cell, _: f32, _: f32) {}
        }
        shareable::<TileGenerator<SharedHeightmap, WorldFilters>>();
    }

    /// A lazy tile shared between threads (as Engine's tile cache shares its entries) is generated
    /// once, and every thread gets that tile.
    #[test]
    fn lazy_tiles_generate_once_across_threads() {
        struct CountingHeightmap(std::sync::atomic::AtomicUsize);
        impl HeightmapSource for CountingHeightmap {
            fn apply_base(&self, cell: &mut Cell, x: f32, z: f32) {
                self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                cell.value = x + z;
            }
            fn rivermap(&self, x: i32, z: i32) -> Arc<Rivermap> {
                Arc::new(Rivermap::new(x, z, Vec::new(), GenWarp::direct()))
            }
            fn apply_rivers(&self, _: &mut Cell, _: f32, _: f32, _: &Rivermap) {}
            fn apply_climate(&self, _: &mut Cell, _: f32, _: f32) {}
        }
        let generator = TileGenerator::new(
            0,
            0,
            0,
            Arc::new(TerrainRegistry::new()),
            WorldGenerator::new(CountingHeightmap(Default::default()), NoFilters),
        );
        let lazy = generator.get_tile(2, -3);
        let barrier = std::sync::Barrier::new(8);
        let tiles: Vec<usize> = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..8)
                .map(|_| {
                    scope.spawn(|| {
                        barrier.wait();
                        std::ptr::from_ref(lazy.get()) as usize
                    })
                })
                .collect();
            handles.into_iter().map(|h| h.join().unwrap()).collect()
        });
        assert!(tiles.iter().all(|&tile| tile == tiles[0]));
        // One 16x16 chunk, each cell's base stage run once.
        let calls = generator
            .generator()
            .heightmap()
            .0
            .load(std::sync::atomic::Ordering::SeqCst);
        assert_eq!(calls, 256);
        assert_eq!(lazy.get().cell(0, 0).value, 32.0 + -48.0);
    }

    /// `TileGeneratorOracle.java`'s output (`./run.sh` writes it to `out.txt`).
    const ORACLE: &str = "\
g 0 132 933dedba501bf8f1 2afe9d3c12774c0d
z 0 0 0 0 134 8ea15ead27a7ffee 4166d8e307e01385
z 0 1 0 0 637 4ecaf1596e70d26 29ad7e3985d0e0bc
z 0 2 0 0 256 6727ea6ac494b891 d25a67c809fc7d0
z 0 3 0 0 8 8f7006778fc0c8d c94e0748cd32a3cd
z 0 4 0 0 388 b8f70426f6f55903 ff2a52d36fd50b5d
b 0 0 1 140 ff293731a98fd032 2afe9d3c12774c0d
y 0 0 1 1 640 fefc13f226a90bf4 29ad7e3985d0e0bc
b 0 0 3 140 ff293731a98fd032 2afe9d3c12774c0d
y 0 0 3 3 20 3a5386a7092fd84 c94e0748cd32a3cd
b 0 0 2 140 ff293731a98fd032 2afe9d3c12774c0d
y 0 0 2 2 264 21f82f8697838 d25a67c809fc7d0
b 0 0 7 140 ff293731a98fd032 2afe9d3c12774c0d
y 0 0 7 2 264 21f82f8697838 d25a67c809fc7d0
b 0 0 100 140 ff293731a98fd032 2afe9d3c12774c0d
y 0 0 100 0 143 1d834f8ce5c4fe1 4166d8e307e01385
b 0 1 1 140 ff293731a98fd032 2afe9d3c12774c0d
y 0 1 1 1 640 fefc13f226a90bf4 29ad7e3985d0e0bc
b 0 1 3 140 ff293731a98fd032 2afe9d3c12774c0d
y 0 1 3 3 20 3a5386a7092fd84 c94e0748cd32a3cd
b 0 1 2 140 ff293731a98fd032 2afe9d3c12774c0d
y 0 1 2 2 264 21f82f8697838 d25a67c809fc7d0
b 0 1 7 140 ff293731a98fd032 2afe9d3c12774c0d
y 0 1 7 2 264 21f82f8697838 d25a67c809fc7d0
b 0 1 100 140 ff293731a98fd032 2afe9d3c12774c0d
y 0 1 100 0 143 1d834f8ce5c4fe1 4166d8e307e01385
e 0 0 0 throws ArithmeticException
e 0 0 -2 throws NullPointerException
e 0 1 0 throws ArithmeticException
e 0 1 -2 throws NullPointerException
l 0 false 0 11 132 933dedba501bf8f1 -1 2 2afe9d3c12774c0d true 0 11 true
m 0 false 0 11 134 8ea15ead27a7ffee 0 0 4166d8e307e01385 true 0 11 true
g 1 397 7245952d37687c55 e76a63e92f8379dd
z 1 0 0 0 387 d49fcfd0bb2413f2 835d8bb2fa3b2820
z 1 1 0 0 1339 11b697d16848b786 567dacf67f971c65
z 1 2 0 0 400 cd67b6f5d8dbd049 245b91724bc38581
z 1 3 0 0 12 375f578adf39af4b c24985dd40cbb39
z 1 4 0 0 780 67386badb7371713 ff26c135e2959741
b 1 0 1 412 430d669402ba99e e76a63e92f8379dd
y 1 0 1 2 416 f2e329f5bab60 245b91724bc38581
b 1 0 3 412 430d669402ba99e e76a63e92f8379dd
y 1 0 3 4 788 4ec03c40ff6e3de ff26c135e2959741
b 1 0 2 412 430d669402ba99e e76a63e92f8379dd
y 1 0 2 3 42 8327cdb50c78cd4 c24985dd40cbb39
b 1 0 7 412 430d669402ba99e e76a63e92f8379dd
y 1 0 7 3 42 8327cdb50c78cd4 c24985dd40cbb39
b 1 0 100 412 430d669402ba99e e76a63e92f8379dd
y 1 0 100 1 1344 fe886cc8d526ea84 567dacf67f971c65
b 1 1 1 412 430d669402ba99e e76a63e92f8379dd
y 1 1 1 2 416 f2e329f5bab60 245b91724bc38581
b 1 1 3 412 430d669402ba99e e76a63e92f8379dd
y 1 1 3 4 788 4ec03c40ff6e3de ff26c135e2959741
b 1 1 2 412 430d669402ba99e e76a63e92f8379dd
y 1 1 2 3 42 8327cdb50c78cd4 c24985dd40cbb39
b 1 1 7 412 430d669402ba99e e76a63e92f8379dd
y 1 1 7 3 42 8327cdb50c78cd4 c24985dd40cbb39
b 1 1 100 412 430d669402ba99e e76a63e92f8379dd
y 1 1 100 1 1344 fe886cc8d526ea84 567dacf67f971c65
e 1 0 0 throws ArithmeticException
e 1 0 -2 throws NullPointerException
e 1 1 0 throws ArithmeticException
e 1 1 -2 throws NullPointerException
l 1 false 0 11 397 7245952d37687c55 3 -5 e76a63e92f8379dd true 0 11 true
m 1 false 0 11 1339 11b697d16848b786 0 0 567dacf67f971c65 true 0 11 true
g 2 64 1dec756bf463a431 40bc3ec1ab771b58
z 2 0 0 0 64 83a19de368360031 bd0ab1842c50e7af
z 2 1 0 0 128 3822735c75797611 cb3b77fd7a9dc52
z 2 2 0 0 4 7984c67e49e48d73 4756c143b5e81e07
z 2 3 0 0 4 ae3ae2c61568fccf a6e0dce0067b12e9
z 2 4 0 0 66 d5ffa2bb28613a2a 51601f1cf26142c
b 2 0 1 66 76f754be1ac50e 40bc3ec1ab771b58
y 2 0 1 3 6 e9bf5b965b23e8 a6e0dce0067b12e9
b 2 0 3 66 76f754be1ac50e 40bc3ec1ab771b58
y 2 0 3 0 66 760d40901c86c2 bd0ab1842c50e7af
b 2 0 2 66 76f754be1ac50e 40bc3ec1ab771b58
y 2 0 2 4 67 8c00650c2da25e 51601f1cf26142c
b 2 0 7 66 76f754be1ac50e 40bc3ec1ab771b58
y 2 0 7 4 67 8c00650c2da25e 51601f1cf26142c
b 2 0 100 66 76f754be1ac50e 40bc3ec1ab771b58
y 2 0 100 2 4 fffdc4acb8ae1c9c 4756c143b5e81e07
b 2 1 1 66 76f754be1ac50e 40bc3ec1ab771b58
y 2 1 1 3 6 e9bf5b965b23e8 a6e0dce0067b12e9
b 2 1 3 66 76f754be1ac50e 40bc3ec1ab771b58
y 2 1 3 0 66 760d40901c86c2 bd0ab1842c50e7af
b 2 1 2 66 76f754be1ac50e 40bc3ec1ab771b58
y 2 1 2 4 67 8c00650c2da25e 51601f1cf26142c
b 2 1 7 66 76f754be1ac50e 40bc3ec1ab771b58
y 2 1 7 4 67 8c00650c2da25e 51601f1cf26142c
b 2 1 100 66 76f754be1ac50e 40bc3ec1ab771b58
y 2 1 100 2 4 fffdc4acb8ae1c9c 4756c143b5e81e07
e 2 0 0 throws ArithmeticException
e 2 0 -2 throws NullPointerException
e 2 1 0 throws ArithmeticException
e 2 1 -2 throws NullPointerException
l 2 false 0 11 64 1dec756bf463a431 0 0 40bc3ec1ab771b58 true 0 11 true
m 2 false 0 11 4 7984c67e49e48d73 0 0 4756c143b5e81e07 true 0 11 true
g 3 326 e3c4cdf7c5eefaee 6f363908c2b37e18
z 3 0 0 0 18 3b45073f941ef450 6ca6c7d6745fb36a
z 3 1 0 0 876 c2eea373f1f90a33 339683019cd47a84
z 3 2 0 0 176 c1be527b1c6e9de9 ee52c23ae4267780
z 3 3 0 0 10 e5baee091137038c c7d462f17993a8db
z 3 4 0 0 490 10079d51f1cdcca3 b4a4cebe6cb0976c
b 3 0 1 338 fd5fd1e315136131 6f363908c2b37e18
y 3 0 1 4 496 36b028e533e6f86 b4a4cebe6cb0976c
b 3 0 3 338 fd5fd1e315136131 6f363908c2b37e18
y 3 0 3 1 880 fec0d50e0c2b5520 339683019cd47a84
b 3 0 2 338 fd5fd1e315136131 6f363908c2b37e18
y 3 0 2 0 30 2e1d26996dc80e4 6ca6c7d6745fb36a
b 3 0 7 338 fd5fd1e315136131 6f363908c2b37e18
y 3 0 7 0 30 2e1d26996dc80e4 6ca6c7d6745fb36a
b 3 0 100 338 fd5fd1e315136131 6f363908c2b37e18
y 3 0 100 3 30 5b1a07cd4c1ae73 c7d462f17993a8db
b 3 1 1 338 fd5fd1e315136131 6f363908c2b37e18
y 3 1 1 4 496 36b028e533e6f86 b4a4cebe6cb0976c
b 3 1 3 338 fd5fd1e315136131 6f363908c2b37e18
y 3 1 3 1 880 fec0d50e0c2b5520 339683019cd47a84
b 3 1 2 338 fd5fd1e315136131 6f363908c2b37e18
y 3 1 2 0 30 2e1d26996dc80e4 6ca6c7d6745fb36a
b 3 1 7 338 fd5fd1e315136131 6f363908c2b37e18
y 3 1 7 0 30 2e1d26996dc80e4 6ca6c7d6745fb36a
b 3 1 100 338 fd5fd1e315136131 6f363908c2b37e18
y 3 1 100 3 30 5b1a07cd4c1ae73 c7d462f17993a8db
e 3 0 0 throws ArithmeticException
e 3 0 -2 throws NullPointerException
e 3 1 0 throws ArithmeticException
e 3 1 -2 throws NullPointerException
l 3 false 0 11 326 e3c4cdf7c5eefaee -7 9 6f363908c2b37e18 true 0 11 true
m 3 false 0 11 10 e5baee091137038c 0 0 c7d462f17993a8db true 0 11 true
g 4 1568 b599986399fb3e01 9a47ad4f7c38f0ee
z 4 0 0 0 1538 5fa0efaac8bcc9e1 c7ce55c3d1993304
z 4 1 0 0 5354 49faaa1e08c16c6d 57f2964297284da4
z 4 2 0 0 1568 cdf03463f768fa01 46ee6896e1099572
z 4 3 0 0 70 ec220899b350f0e4 28f7541d10beeeef
z 4 4 0 0 2733 fb4116edd58af70f 970936609b0cd3ac
b 4 0 1 1648 10e3e60eaa993958 9a47ad4f7c38f0ee
y 4 0 1 0 1648 109898f04353cd88 c7ce55c3d1993304
b 4 0 3 1648 10e3e60eaa993958 9a47ad4f7c38f0ee
y 4 0 3 2 1648 74713a7c086b10 46ee6896e1099572
b 4 0 2 1648 10e3e60eaa993958 9a47ad4f7c38f0ee
y 4 0 2 1 5376 fd76ddefa49ba6a0 57f2964297284da4
b 4 0 7 1648 10e3e60eaa993958 9a47ad4f7c38f0ee
y 4 0 7 1 5376 fd76ddefa49ba6a0 57f2964297284da4
b 4 0 100 1648 10e3e60eaa993958 9a47ad4f7c38f0ee
y 4 0 100 4 2783 13b00ea32c87c9a8 970936609b0cd3ac
b 4 1 1 1648 10e3e60eaa993958 9a47ad4f7c38f0ee
y 4 1 1 0 1648 109898f04353cd88 c7ce55c3d1993304
b 4 1 3 1648 10e3e60eaa993958 9a47ad4f7c38f0ee
y 4 1 3 2 1648 74713a7c086b10 46ee6896e1099572
b 4 1 2 1648 10e3e60eaa993958 9a47ad4f7c38f0ee
y 4 1 2 1 5376 fd76ddefa49ba6a0 57f2964297284da4
b 4 1 7 1648 10e3e60eaa993958 9a47ad4f7c38f0ee
y 4 1 7 1 5376 fd76ddefa49ba6a0 57f2964297284da4
b 4 1 100 1648 10e3e60eaa993958 9a47ad4f7c38f0ee
y 4 1 100 4 2783 13b00ea32c87c9a8 970936609b0cd3ac
e 4 0 0 throws ArithmeticException
e 4 0 -2 throws NullPointerException
e 4 1 0 throws ArithmeticException
e 4 1 -2 throws NullPointerException
l 4 false 0 11 1568 b599986399fb3e01 1 -1 9a47ad4f7c38f0ee true 0 11 true
m 4 false 0 11 2733 fb4116edd58af70f 0 0 970936609b0cd3ac true 0 11 true
g 5 137 d84abfe0adb754b 4c8e54bd50b0358b
z 5 0 0 0 134 8ea15ead27a7ffee f7d66956f95d7b09
z 5 1 0 0 637 4ecaf1596e70d26 19eab031c83bcbf7
z 5 2 0 0 256 6727ea6ac494b891 26501176026dc54e
z 5 3 0 0 8 8f7006778fc0c8d c0702424e84aeb1e
z 5 4 0 0 388 b8f70426f6f55903 2b01bed53f599881
b 5 0 1 143 e47d7e7dd561fa 4c8e54bd50b0358b
y 5 0 1 1 640 fefc13f226a90bf4 19eab031c83bcbf7
b 5 0 3 143 e47d7e7dd561fa 4c8e54bd50b0358b
y 5 0 3 3 20 3a5386a7092fd84 c0702424e84aeb1e
b 5 0 2 143 e47d7e7dd561fa 4c8e54bd50b0358b
y 5 0 2 2 264 21f82f8697838 26501176026dc54e
b 5 0 7 143 e47d7e7dd561fa 4c8e54bd50b0358b
y 5 0 7 2 264 21f82f8697838 26501176026dc54e
b 5 0 100 143 e47d7e7dd561fa 4c8e54bd50b0358b
y 5 0 100 0 143 1d834f8ce5c4fe1 f7d66956f95d7b09
b 5 1 1 143 e47d7e7dd561fa 4c8e54bd50b0358b
y 5 1 1 1 640 fefc13f226a90bf4 19eab031c83bcbf7
b 5 1 3 143 e47d7e7dd561fa 4c8e54bd50b0358b
y 5 1 3 3 20 3a5386a7092fd84 c0702424e84aeb1e
b 5 1 2 143 e47d7e7dd561fa 4c8e54bd50b0358b
y 5 1 2 2 264 21f82f8697838 26501176026dc54e
b 5 1 7 143 e47d7e7dd561fa 4c8e54bd50b0358b
y 5 1 7 2 264 21f82f8697838 26501176026dc54e
b 5 1 100 143 e47d7e7dd561fa 4c8e54bd50b0358b
y 5 1 100 0 143 1d834f8ce5c4fe1 f7d66956f95d7b09
e 5 0 0 throws ArithmeticException
e 5 0 -2 throws NullPointerException
e 5 1 0 throws ArithmeticException
e 5 1 -2 throws NullPointerException
l 5 false 0 11 137 d84abfe0adb754b 268435456 -268435457 4c8e54bd50b0358b true 0 11 true
m 5 false 0 11 134 8ea15ead27a7ffee 0 0 f7d66956f95d7b09 true 0 11 true
s 0 0 1 -1 7 -8 1000 -1000 2147483647 -2147483648
s 1 0 0 -1 3 -4 500 -500 1073741823 -1073741824
s 3 0 0 -1 0 -1 125 -125 268435455 -268435456
s 31 0 0 -1 0 -1 0 -1 0 -1
s 32 0 1 -1 7 -8 1000 -1000 2147483647 -2147483648
s 33 0 0 -1 3 -4 500 -500 1073741823 -1073741824
s -1 0 0 -1 0 -1 0 -1 0 -1
s -31 0 0 -1 3 -4 500 -500 1073741823 -1073741824
";
}
