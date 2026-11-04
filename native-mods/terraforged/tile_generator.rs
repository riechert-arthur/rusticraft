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
    terraforged_climate::Climate,
    terraforged_concurrent::{Cache, CacheEntry, DisposeListener, LazyCallable, ThreadPool},
    terraforged_context::GeneratorContext,
    terraforged_engine::{Cell, TerrainRegistry},
    terraforged_filters::WorldFilters,
    terraforged_heightmap::Heightmap,
    terraforged_river::{Rivermap, current_time_millis},
    terraforged_tile::{BatchChunk, ChunkReader, GenChunk, Tile},
};
use std::sync::{Arc, PoisonError, RwLock, Weak};

/// A tile's dispose listener ([`TileGenerator::set_listener`]).
type TileListener = Weak<dyn DisposeListener<Tile>>;

/// The stages of Engine's `Heightmap` that `Tile.generate(Heightmap ...)` calls, in the order the
/// tile fills run them: `applyBase`, `getContinent().getRivermap(x, z)` (when the cell's continent
/// cell has no river map yet), `applyRivers` and `applyClimate`. The generator takes any
/// implementation; `Heightmap` is the real one.
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

impl<H: HeightmapSource + ?Sized> HeightmapSource for Arc<H> {
    fn apply_base(&self, cell: &mut Cell, x: f32, z: f32) {
        (**self).apply_base(cell, x, z);
    }

    fn rivermap(&self, x: i32, z: i32) -> Arc<Rivermap> {
        (**self).rivermap(x, z)
    }

    fn apply_rivers(&self, cell: &mut Cell, x: f32, z: f32, rivermap: &Rivermap) {
        (**self).apply_rivers(cell, x, z, rivermap);
    }

    fn apply_climate(&self, cell: &mut Cell, x: f32, z: f32) {
        (**self).apply_climate(cell, x, z);
    }
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

impl<F: TileFilters + ?Sized> TileFilters for Arc<F> {
    fn apply(&self, tile: &mut Tile, optional: bool) {
        (**self).apply(tile, optional);
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

/// The [`WorldGenerator`] a [`WorldGeneratorFactory`] supplies: its shared heightmap and filters.
pub type TerraForgedWorldGenerator = WorldGenerator<Arc<Heightmap>, Arc<WorldFilters>>;

/// Engine 0.3.0's `WorldGeneratorFactory` (`com.terraforged.engine.world`, a `Supplier<WorldGenerator>`): the
/// world's [`Heightmap`] and [`WorldFilters`], built once from the generator context and shared by every
/// [`WorldGenerator`] [`Self::get`] returns, plus the world's registry (Engine's static `TerrainType`) that the
/// heightmap registered its composite terrains into and that the tiles' cells start from.
///
/// [`Self::new`] is `new WorldGeneratorFactory(context)`; Engine's lazy `context.worldGenerator` is
/// [`GeneratorContext::world_generator`]. Engine's other lazy supplier, `context.cache` (`createCache`:
/// `TileGenerator.builder().pool(ThreadPools.createDefault()).factory(factory).size(3, 1).build().cached()`), is
/// not ported: only Engine's `WorldLookup` reads it.
pub struct WorldGeneratorFactory {
    terrains: Arc<TerrainRegistry>,
    heightmap: Arc<Heightmap>,
    filters: Arc<WorldFilters>,
}

impl WorldGeneratorFactory {
    /// `new WorldGeneratorFactory(context)`: `new Heightmap(context)`, which draws from `context.seed` (17 draws,
    /// 20 for `MULTI_IMPROVED` continents) and registers its composite terrains into `context.terrain`, then
    /// `new WorldFilters(context)`, which draws nothing.
    ///
    /// # Panics
    ///
    /// As [`Heightmap::new`], if anything else already holds `context.terrain`.
    pub fn new(context: &mut GeneratorContext) -> Self {
        let heightmap = Heightmap::new(context);
        Self::with_heightmap(context, heightmap)
    }

    /// `new WorldGeneratorFactory(context, heightmap)`: the given heightmap (built from this context, so that it
    /// registered into `context.terrain`) and `new WorldFilters(context)`.
    pub fn with_heightmap(context: &GeneratorContext, heightmap: Heightmap) -> Self {
        Self {
            terrains: Arc::clone(&context.terrain),
            heightmap: Arc::new(heightmap),
            filters: Arc::new(WorldFilters::from_context(context)),
        }
    }

    /// The world's registry (the context's), for [`TileGeneratorBuilder::factory`].
    pub fn terrains(&self) -> &Arc<TerrainRegistry> {
        &self.terrains
    }

    /// `getHeightmap()`.
    pub fn heightmap(&self) -> &Arc<Heightmap> {
        &self.heightmap
    }

    /// `getClimate()`: `getHeightmap().getClimate()`.
    pub fn climate(&self) -> &Climate {
        self.heightmap.climate()
    }

    /// `getFilters()`.
    pub fn filters(&self) -> &Arc<WorldFilters> {
        &self.filters
    }

    /// `get()`: `new WorldGenerator(heightmap, filters)`, sharing this factory's heightmap and filters.
    pub fn get(&self) -> TerraForgedWorldGenerator {
        WorldGenerator::new(Arc::clone(&self.heightmap), Arc::clone(&self.filters))
    }
}

/// Engine 0.3.0's `TileGenerator` (`com.terraforged.engine.tile.gen`): the size factor and border
/// (in chunks) of its tiles, the batch size, the world's registry the tiles' cells start from (in
/// place of Engine's `TileResources` pools), the world generator, and which of Engine's three
/// generator classes it is ([`Self::builder`] picks it as `Builder.build()` does; [`Self::new`]
/// makes the plain one). Engine's `TileGeneratorBatched` and `TileGeneratorStriped` are also the
/// `generate_batched_*` and `generate_striped_*` methods; [`Self::cached`] is its `cached()` and
/// [`Self::asynchronous`] its `async()`.
pub struct TileGenerator<H, F> {
    factor: i32,
    border: i32,
    batch_size: i32,
    kind: Kind,
    /// The batch threads; `None` for Engine's default pool, resolved when a batched fill runs.
    batch_threads: Option<usize>,
    /// Engine's `threadPool` as the tile cache's entries use it: the builder's
    /// `ThreadPool::create(size)`, else [`ThreadPool::NONE`] (`Builder.getOrDefaultPool`).
    pool: ThreadPool,
    terrains: Arc<TerrainRegistry>,
    generator: WorldGenerator<H, F>,
    /// Engine's `listener`, which every new tile takes ([`Self::set_listener`]).
    listener: RwLock<Option<TileListener>>,
}

/// The Engine class `TileGenerator.Builder.build()` returns, which decides what `generateRegion`
/// runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// `TileGenerator`: the whole tile filled at once.
    Plain,
    /// `TileGeneratorBatched`: `Tile.generateArea` with the batch size.
    Batched,
    /// `TileGeneratorStriped`: `Tile.generateAreaStriped` with the batch size.
    Striped,
}

/// Engine's `TileGenerator.Builder` (from [`TileGenerator::builder`]): size factor 0, border 0,
/// batch size 0, not striped and no thread pool until set. Like Engine's builder it checks no
/// argument; [`Self::build`] panics without a [`Self::factory`], where Engine throws
/// `NullPointerException`.
pub struct TileGeneratorBuilder<H, F> {
    factor: i32,
    border: i32,
    batch_size: i32,
    striped: bool,
    pool: Option<i32>,
    factory: Option<(Arc<TerrainRegistry>, WorldGenerator<H, F>)>,
}

impl<H, F> TileGeneratorBuilder<H, F>
where
    H: HeightmapSource,
    F: TileFilters,
{
    /// `size(factor, border)`: `factor(factor).border(border)`.
    #[must_use]
    pub const fn size(self, factor: i32, border: i32) -> Self {
        self.factor(factor).border(border)
    }

    /// `factor(factor)`: the tiles' size factor (`1 << factor` chunks a side).
    #[must_use]
    pub const fn factor(mut self, factor: i32) -> Self {
        self.factor = factor;
        self
    }

    /// `border(border)`: the tiles' border, in chunks.
    #[must_use]
    pub const fn border(mut self, border: i32) -> Self {
        self.border = border;
        self
    }

    /// `pool(ThreadPools.create(size))`: the batches run on `max(2, size)` threads, the batch
    /// threads of Engine's `BatchingThreadPool.of(size)`. Without a pool (Engine's
    /// `ThreadPools.NONE`, a synchronous batcher) every batch runs on the calling thread, in order.
    /// The tile cache's entries ([`TileGenerator::cached`]) run on `ThreadPool::create(size)`, else
    /// on `ThreadPool::NONE`.
    #[must_use]
    pub const fn pool(mut self, size: i32) -> Self {
        self.pool = Some(size);
        self
    }

    /// `factory(factory)`: the world generator the factory supplies, and the world's registry the
    /// tiles' cells start from.
    #[must_use]
    pub fn factory(
        mut self,
        terrains: Arc<TerrainRegistry>,
        generator: WorldGenerator<H, F>,
    ) -> Self {
        self.factory = Some((terrains, generator));
        self
    }

    /// `batch(batchSize)`.
    #[must_use]
    pub const fn batch(mut self, batch_size: i32) -> Self {
        self.batch_size = batch_size;
        self
    }

    /// `striped()`.
    #[must_use]
    pub const fn striped(mut self) -> Self {
        self.striped = true;
        self
    }

    /// `build()`: `TileGeneratorStriped` for a positive batch size with `striped()`,
    /// `TileGeneratorBatched` for a positive batch size without it, the plain `TileGenerator`
    /// otherwise (whatever `striped()` and the pool say). Panics without a factory.
    pub fn build(self) -> TileGenerator<H, F> {
        let kind = match (self.batch_size > 0, self.striped) {
            (false, _) => Kind::Plain,
            (true, false) => Kind::Batched,
            (true, true) => Kind::Striped,
        };
        let batch_threads = Some(self.pool.map_or(1, |size| size.max(2) as usize));
        let pool = self.pool.map_or(ThreadPool::NONE, ThreadPool::create);
        let (terrains, generator) = self.factory.expect(
            "TileGenerator.Builder.build() without a factory: Engine throws NullPointerException",
        );
        TileGenerator {
            factor: self.factor,
            border: self.border,
            batch_size: self.batch_size,
            kind,
            batch_threads,
            pool,
            terrains,
            generator,
            listener: RwLock::new(None),
        }
    }
}

// Engine forces the factory and runs its tile generators and caches on other threads: they must be shareable.
const _: fn() = || {
    fn shareable<T: Send + Sync + 'static>() {}
    shareable::<WorldGeneratorFactory>();
    shareable::<TileGenerator<Arc<Heightmap>, Arc<WorldFilters>>>();
};

impl TileGeneratorBuilder<Arc<Heightmap>, Arc<WorldFilters>> {
    /// `factory(factory)` with a real [`WorldGeneratorFactory`]: Engine's `TileGenerator` takes
    /// `factory.get()`; the tiles' cells start from the factory's registry.
    #[must_use]
    pub fn world_factory(self, factory: &WorldGeneratorFactory) -> Self {
        self.factory(Arc::clone(factory.terrains()), factory.get())
    }
}

impl<H, F> TileGenerator<H, F>
where
    H: HeightmapSource,
    F: TileFilters,
{
    /// The plain generator (Engine's `TileGenerator` class) with `size(factor, border)`, a factory
    /// supplying `generator` and the batch size, which only the `generate_batched_*` and
    /// `generate_striped_*` methods use (on Engine's default pool, `max(2, processors)` threads);
    /// `terrains` is the world's registry. Engine's builder makes the plain class only for batch
    /// sizes of 0 and less: use [`Self::builder`] for its choice. Its tile cache's entries run on
    /// [`ThreadPool::NONE`], as without a builder `pool`.
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
            kind: Kind::Plain,
            batch_threads: None,
            pool: ThreadPool::NONE,
            terrains,
            generator,
            listener: RwLock::new(None),
        }
    }

    /// `TileGenerator.builder()`.
    pub const fn builder() -> TileGeneratorBuilder<H, F> {
        TileGeneratorBuilder {
            factor: 0,
            border: 0,
            batch_size: 0,
            striped: false,
            pool: None,
            factory: None,
        }
    }

    /// `getGenerator()`.
    pub const fn generator(&self) -> &WorldGenerator<H, F> {
        &self.generator
    }

    /// The builder's `batchSize`, which the plain generator's `generateRegion` ignores.
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
    ) -> LazyCallable<Tile, impl Fn() -> Tile + '_>
    where
        H: Sync,
    {
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
    ) -> LazyCallable<Tile, impl Fn() -> Tile + '_>
    where
        H: Sync,
    {
        LazyCallable::new(move || self.generate_zoomed_region(center_x, center_z, zoom, filter))
    }

    /// `generateRegion(regionX, regionZ)` of the generator's Engine class: the batched and striped
    /// ones run [`Self::generate_batched_region`] or [`Self::generate_striped_region`] with the
    /// batch size. The plain one fills an empty region ([`Self::create_empty_region`]) whole by
    /// [`Tile::generate_heightmap`] with the heightmap's stages, then runs `postProcess(tile)`:
    /// the filters with the optional ones (`apply(tile, true)`), which see every cell of the tile,
    /// border included, only after the fill.
    pub fn generate_region(&self, region_x: i32, region_z: i32) -> Tile
    where
        H: Sync,
    {
        match self.kind {
            Kind::Plain => {}
            Kind::Batched => {
                return self.generate_batched_region(region_x, region_z, self.batch_size);
            }
            Kind::Striped => {
                return self.generate_striped_region(region_x, region_z, self.batch_size);
            }
        }
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

    /// `generateRegion(centerX, centerZ, zoom, filter)` of the generator's Engine class: the
    /// batched and striped ones run [`Self::generate_batched_zoomed_region`] or
    /// [`Self::generate_striped_zoomed_region`] with the batch size. The plain one fills region
    /// (0, 0) by [`Tile::generate_zoomed_heightmap`] with the center as its offset (uncached, as
    /// Engine's `tile.generate(heightmap, centerX, centerZ, zoom)`), then runs
    /// `postProcess(tile, filter)`: the filters, the optional ones only when `filter` is set. The
    /// filters are seeded with region (0, 0) whatever the center.
    pub fn generate_zoomed_region(
        &self,
        center_x: f32,
        center_z: f32,
        zoom: f32,
        filter: bool,
    ) -> Tile
    where
        H: Sync,
    {
        let count = self.batch_size;
        match self.kind {
            Kind::Plain => {}
            Kind::Batched => {
                return self
                    .generate_batched_zoomed_region(center_x, center_z, zoom, filter, count);
            }
            Kind::Striped => {
                return self
                    .generate_striped_zoomed_region(center_x, center_z, zoom, filter, count);
            }
        }
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
    /// region filled by [`Tile::generate_area`] (`batch_count` squared batches of chunks on the
    /// builder's pool threads, else [`batch_workers`], each chunk as [`drive_chunk`]), then the
    /// filters with the optional ones. The cells equal the plain generator's for any batch count
    /// and thread count; only the river-map lookups differ, as each chunk starts without one.
    /// Engine's builder never makes a batched generator for a count below 1; called with one, it
    /// throws, and so does this: `ArithmeticException` for 0 (`getBatchSize`), and for a negative
    /// count, which fills no cell, a `NullPointerException` once the filters read the cells it
    /// never created. A stage's panic is re-raised.
    pub fn generate_batched_region(&self, region_x: i32, region_z: i32, batch_count: i32) -> Tile
    where
        H: Sync,
    {
        check_batch_count(batch_count);
        let heightmap = self.generator.heightmap();
        let mut tile = self.create_empty_region(region_x, region_z);
        tile.generate_area(batch_count, self.threads(), |chunk| {
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
        tile.generate_area(batch_count, self.threads(), |chunk| {
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
        tile.generate_area_striped(sections, self.threads(), |chunk| {
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
        tile.generate_area_striped(sections, self.threads(), |chunk| {
            drive_chunk(chunk, heightmap, zoom);
        });
        self.generator.filters().apply(&mut tile, filter);
        tile
    }

    /// `createEmptyRegion(regionX, regionZ)`: a fresh tile of the generator's size factor and
    /// border, its cells from the world's registry, with the generator's current listener (zoomed
    /// tiles too, as region (0, 0)).
    pub fn create_empty_region(&self, region_x: i32, region_z: i32) -> Tile {
        let listener = self
            .listener
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        Tile::with_listener(
            region_x,
            region_z,
            self.factor,
            self.border,
            &self.terrains,
            listener,
        )
    }

    /// `setListener(listener)`: the dispose listener of the tiles made from now on (tiles already
    /// made keep theirs). [`TileCache::new`] sets itself; without it, a tile's `dispose` tells no
    /// one.
    pub fn set_listener(&self, listener: TileListener) {
        *self
            .listener
            .write()
            .unwrap_or_else(PoisonError::into_inner) = Some(listener);
    }

    /// The batch threads: the builder's pool, else Engine's default pool ([`batch_workers`]).
    fn threads(&self) -> usize {
        self.batch_threads.unwrap_or_else(batch_workers)
    }
}

impl<H, F> TileGenerator<H, F>
where
    H: HeightmapSource + Send + Sync + 'static,
    F: TileFilters + Send + Sync + 'static,
{
    /// `cached()`: `new TileCache(this, threadPool)`, the generator's tile cache on its pool. The
    /// cache shares the generator, which it keeps alive for its entries' tasks.
    pub fn cached(self: Arc<Self>) -> TileCache<H, F> {
        let pool = self.pool.clone();
        TileCache::new(self, pool)
    }

    /// `async()`: `new TileGeneratorAsync(this, threadPool)`, the generator's async factory on its
    /// pool.
    pub fn asynchronous(self: Arc<Self>) -> AsyncTileGenerator<H, F> {
        let pool = self.pool.clone();
        AsyncTileGenerator::new(self, pool)
    }
}

/// Engine 0.3.0's `TileGeneratorAsync` (`com.terraforged.engine.tile.gen`): a [`TileGenerator`]
/// whose `getTile` overloads hand the generator's lazy tile to a [`ThreadPool`]
/// (`LazyCallable.callAsync(generator.getTile(...), threadPool)`). Each call submits its own task:
/// unlike [`TileCache`], nothing is shared between calls for one region.
///
/// The returned [`CacheEntry`] behaves as Engine's `FutureAdapter` over `threadPool.submit`: on a
/// [`ThreadPool::create`] pool the tile starts on a task thread at once, `is_done` is `false` until
/// it ends, and `get`/`call` wait for it; a panicked task makes every later `get`/`call` panic
/// without running it again (Engine: `NullPointerException`). On [`ThreadPool::NONE`] it is done
/// at once and the tile is made on the first `get`'s thread, again after a panic. Its timestamp,
/// which Engine's `FutureAdapter` lacks, is unused. The tiles are the wrapped generator's, with the
/// dispose listener it has when the task runs ([`TileGenerator::create_empty_region`]).
pub struct AsyncTileGenerator<H, F> {
    generator: Arc<TileGenerator<H, F>>,
    thread_pool: ThreadPool,
}

impl<H, F> AsyncTileGenerator<H, F>
where
    H: HeightmapSource + Send + Sync + 'static,
    F: TileFilters + Send + Sync + 'static,
{
    /// `new TileGeneratorAsync(generator, threadPool)`: any pool, not only the generator's.
    pub const fn new(generator: Arc<TileGenerator<H, F>>, thread_pool: ThreadPool) -> Self {
        Self {
            generator,
            thread_pool,
        }
    }

    /// `chunkToRegion(i)`: the generator's.
    pub fn chunk_to_region(&self, chunk: i32) -> i32 {
        self.generator.chunk_to_region(chunk)
    }

    /// `setListener(listener)`: the generator's [`TileGenerator::set_listener`].
    pub fn set_listener(&self, listener: TileListener) {
        self.generator.set_listener(listener);
    }

    /// `getTile(regionX, regionZ)`: [`TileGenerator::generate_region`] on the pool.
    pub fn get_tile(&self, region_x: i32, region_z: i32) -> CacheEntry<'static, Tile> {
        let generator = Arc::clone(&self.generator);
        CacheEntry::compute_async(
            move || generator.generate_region(region_x, region_z),
            &self.thread_pool,
        )
    }

    /// `getTile(centerX, centerZ, zoom, filter)`: [`TileGenerator::generate_zoomed_region`] on the
    /// pool.
    pub fn get_zoomed_tile(
        &self,
        center_x: f32,
        center_z: f32,
        zoom: f32,
        filter: bool,
    ) -> CacheEntry<'static, Tile> {
        let generator = Arc::clone(&self.generator);
        CacheEntry::compute_async(
            move || generator.generate_zoomed_region(center_x, center_z, zoom, filter),
            &self.thread_pool,
        )
    }

    /// `async()`: this factory.
    pub const fn asynchronous(&self) -> &Self {
        self
    }

    /// `cached()`: `new TileCache(generator, threadPool)`, a cache over the wrapped generator (not
    /// this factory) on this factory's pool; it becomes the generator's dispose listener.
    pub fn cached(&self) -> TileCache<H, F> {
        TileCache::new(Arc::clone(&self.generator), self.thread_pool.clone())
    }

    /// The wrapped generator.
    pub const fn generator(&self) -> &Arc<TileGenerator<H, F>> {
        &self.generator
    }
}

/// Engine 0.3.0's `TileCache` (`com.terraforged.engine.tile.gen`) with `TileProvider`'s lookups:
/// an expiring [`Cache`] named `"TileCache"` (capacity 256, a 60 second lifetime swept every 20
/// seconds) of [`CacheEntry`]s keyed by the region's packed id ([`Tile::region_id_for`]). A miss
/// runs `computeCacheEntry`: [`CacheEntry::compute_async`] of the generator's `getTile(regionX,
/// regionZ)` on the cache's pool, so concurrent lookups of one region share one entry and its tile
/// is generated once (on [`ThreadPool::NONE`], on the first `get`'s thread). The tasks own the
/// generator through an `Arc` (Engine's `CallableTile` keeps a reference to it).
///
/// [`Self::get_chunk`] is `TileProvider.getChunk`. The cache is its generator's dispose listener
/// (`generator.setListener(this)`): a tile made after that, once disposed ([`Tile::dispose`]),
/// runs `onDispose`, `cache.remove(tile.getRegionId())`, which removes whatever entry the region
/// has now (a newer tile's too) and closes it ([`Tile::close`]) outside the cache's lock. Expiry
/// and eviction drop an entry without closing it or telling the listener, as Engine's do. Engine's
/// `Tile.close` hands the tile's cells back to `TileResources` pools, which are not ported: here
/// the cells are freed when the last holder of the entry drops it.
pub struct TileCache<H, F> {
    can_queue: bool,
    generator: Arc<TileGenerator<H, F>>,
    thread_pool: ThreadPool,
    cache: Arc<Cache<CacheEntry<'static, Tile>>>,
    clock: fn() -> i64,
}

/// `TileCache.onDispose(tile)`: `cache.remove(tile.getRegionId())`.
impl DisposeListener<Tile> for Cache<CacheEntry<'static, Tile>> {
    fn on_dispose(&self, tile: &Tile) {
        self.remove(tile.region_id());
    }
}

/// `TileCache.onDispose(tile)`, public in Engine (`TileProvider` is a `Disposable.Listener`): the
/// cache's own, which its generator's tiles call.
impl<H: Send + Sync, F: Send + Sync> DisposeListener<Tile> for TileCache<H, F> {
    fn on_dispose(&self, tile: &Tile) {
        self.cache.on_dispose(tile);
    }
}

impl<H, F> TileCache<H, F>
where
    H: HeightmapSource + Send + Sync + 'static,
    F: TileFilters + Send + Sync + 'static,
{
    /// `QUEUING_MIN_POOL_SIZE`: [`Self::queue_region`] runs only on pools larger than this.
    pub const QUEUING_MIN_POOL_SIZE: i32 = 4;

    /// `new TileCache(generator, threadPool)`: queues only when `threadPool.size() > 4`, and
    /// becomes the generator's dispose listener ([`TileGenerator::set_listener`]).
    pub fn new(generator: Arc<TileGenerator<H, F>>, thread_pool: ThreadPool) -> Self {
        Self::with_clock(generator, thread_pool, current_time_millis)
    }

    /// [`Self::new`] reading the time (for the cache's sweeps and its entries' timestamps) from
    /// `clock` in place of `System.currentTimeMillis()`.
    pub fn with_clock(
        generator: Arc<TileGenerator<H, F>>,
        thread_pool: ThreadPool,
        clock: fn() -> i64,
    ) -> Self {
        let cache = Arc::new(Cache::with_clock("TileCache", 256, 60_000, 20_000, clock));
        let listener = Arc::downgrade(&cache);
        generator.set_listener(listener);
        Self {
            can_queue: thread_pool.size() > Self::QUEUING_MIN_POOL_SIZE,
            generator,
            thread_pool,
            cache,
            clock,
        }
    }

    /// `chunkToRegion(coord)`: the generator's.
    pub fn chunk_to_region(&self, coord: i32) -> i32 {
        self.generator.chunk_to_region(coord)
    }

    /// `get(id)`: the region's entry, if cached; never generates or sweeps.
    pub fn get(&self, id: i64) -> Option<Arc<CacheEntry<'static, Tile>>> {
        self.cache.get(id)
    }

    /// `getOrCompute(id)`: the region's entry, made by `computeCacheEntry` on a miss (after a due
    /// sweep, see [`Cache::compute_if_absent`]); does not wait for the tile.
    pub fn get_or_compute(&self, id: i64) -> Arc<CacheEntry<'static, Tile>> {
        self.cache
            .compute_if_absent(id, |id| self.compute_cache_entry(id))
    }

    /// `queueChunk(chunkX, chunkZ)`: [`Self::queue_region`] of the chunk's region.
    pub fn queue_chunk(&self, chunk_x: i32, chunk_z: i32) {
        if self.can_queue {
            self.queue_region(self.chunk_to_region(chunk_x), self.chunk_to_region(chunk_z));
        }
    }

    /// `queueRegion(regionX, regionZ)`: starts the region's tile (`getOrCompute`) when the pool
    /// is larger than [`Self::QUEUING_MIN_POOL_SIZE`]; does nothing otherwise.
    pub fn queue_region(&self, region_x: i32, region_z: i32) {
        if self.can_queue {
            self.get_or_compute(Tile::region_id_for(region_x, region_z));
        }
    }

    /// `getTile(id)`: `getOrCompute(id).get()`, which stamps the entry and waits for (or, on
    /// [`ThreadPool::NONE`], runs) its tile. Returns the entry, whose `call()` then reads the
    /// tile without stamping it again.
    pub fn get_tile(&self, id: i64) -> Arc<CacheEntry<'static, Tile>> {
        let entry = self.get_or_compute(id);
        entry.get();
        entry
    }

    /// `getTile(regionX, regionZ)`: [`Self::get_tile`] of the packed region id.
    pub fn get_tile_at(&self, region_x: i32, region_z: i32) -> Arc<CacheEntry<'static, Tile>> {
        self.get_tile(Tile::region_id_for(region_x, region_z))
    }

    /// `getTileIfPresent(id)`: the cached entry once `isDone`, after its `get` (which stamps it);
    /// `None` when absent or still running. On [`ThreadPool::NONE`] an entry is done at once, so
    /// this runs a cached region's tile that no one has got yet, as Engine does.
    pub fn get_tile_if_present(&self, id: i64) -> Option<Arc<CacheEntry<'static, Tile>>> {
        let entry = self.get(id).filter(|entry| entry.is_done())?;
        entry.get();
        Some(entry)
    }

    /// `getTileIfPresent(regionX, regionZ)`.
    pub fn get_tile_if_present_at(
        &self,
        region_x: i32,
        region_z: i32,
    ) -> Option<Arc<CacheEntry<'static, Tile>>> {
        self.get_tile_if_present(Tile::region_id_for(region_x, region_z))
    }

    /// `getChunk(chunkX, chunkZ)`: `getTile(chunkToRegion(chunkX), chunkToRegion(chunkZ))`, which
    /// stamps the region's entry and waits for (or, on [`ThreadPool::NONE`], runs) its tile, then
    /// that tile's `getChunk(chunkX, chunkZ)` ([`Tile::chunk`]), which opens the chunk
    /// ([`Tile::open_chunk`]; [`CachedChunk::close`] closes it). `None` where Engine throws a
    /// `NullPointerException`: the tile has no chunk in that slot (every tile the generator makes
    /// fills all its slots, so this does not happen here). Panics where [`Tile::chunk`] does.
    pub fn get_chunk(&self, chunk_x: i32, chunk_z: i32) -> Option<CachedChunk> {
        let region_x = self.chunk_to_region(chunk_x);
        let region_z = self.chunk_to_region(chunk_z);
        let tile = self.get_tile_at(region_x, region_z);
        let chunk = tile.call().chunk(chunk_x, chunk_z)?.chunk();
        tile.call().open_chunk();
        Some(CachedChunk {
            tile,
            chunk_x,
            chunk_z,
            chunk,
        })
    }

    /// `computeCacheEntry(id)`: `CacheEntry.computeAsync(generator.getTile(regionX, regionZ),
    /// threadPool)`; the task is `CallableTile`'s, [`TileGenerator::generate_region`].
    fn compute_cache_entry(&self, id: i64) -> CacheEntry<'static, Tile> {
        let (region_x, region_z) = (Tile::region_x_from(id), Tile::region_z_from(id));
        let generator = Arc::clone(&self.generator);
        CacheEntry::compute_async_with_clock(
            move || generator.generate_region(region_x, region_z),
            &self.thread_pool,
            self.clock,
        )
        .closing(Tile::close)
    }
}

/// The `ChunkReader` [`TileCache::get_chunk`] returns: Engine's `GenChunk` is an inner class of its
/// `Tile`, so the reader keeps the tile alive after the cache drops it; this one holds the
/// tile's cache entry. [`Self::reader`] reads it as [`Tile::chunk`]'s [`ChunkReader`] does
/// (`getCell`, `iterate`, `visit` with Engine's swapped bounds), without stamping the entry, as
/// Engine's reads do not. It is open from [`TileCache::get_chunk`] until [`Self::close`], which, as
/// in Engine, is up to the caller: a reader dropped unclosed keeps its tile from closing (Engine's
/// `WorldLookup` never closes its readers either).
pub struct CachedChunk {
    tile: Arc<CacheEntry<'static, Tile>>,
    chunk_x: i32,
    chunk_z: i32,
    chunk: GenChunk,
}

impl CachedChunk {
    /// The chunk's position (`getChunkX`, `getBlockX`...).
    pub const fn chunk(&self) -> GenChunk {
        self.chunk
    }

    /// The tile the chunk is in.
    pub fn tile(&self) -> &Tile {
        self.tile.call()
    }

    /// The chunk's [`ChunkReader`].
    pub fn reader(&self) -> ChunkReader<'_> {
        let chunk = self.tile().chunk(self.chunk_x, self.chunk_z);
        chunk.expect("a cached tile keeps its chunks")
    }

    /// `close()`: `GenChunk.close`, one open reader of the tile fewer ([`Tile::close_chunk`]).
    /// Like Engine's, it does not check whether this reader was closed already.
    pub fn close(&self) {
        self.tile().close_chunk();
    }

    /// `dispose()`: `GenChunk.dispose`, the tile's [`Tile::dispose`].
    pub fn dispose(&self) {
        self.tile().dispose();
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
        terraforged_concurrent::ExpiringEntry,
        terraforged_engine::{Levels, Terrain, WorldControlPoints},
        terraforged_filters::FilterSettings,
        terraforged_river::GenWarp,
    };
    use super::*;
    use std::panic::AssertUnwindSafe;
    use std::sync::atomic::{AtomicBool, AtomicI32, AtomicI64, Ordering};
    use std::sync::{Mutex, mpsc};

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
        let total = tile.block_size().total;
        let cells = (0..total).flat_map(|z| (0..total).map(move |x| (x, z)));
        cells.fold(17, |f, (x, z)| {
            fold_cell(f, tile.raw_cell(x, z).expect("backing cell"))
        })
    }

    /// One cell into `fold`, as the oracles' `fold`/`mixCell`.
    fn fold_cell(mut f: i64, cell: &Cell) -> i64 {
        let bits = |value: f32| {
            if value.is_nan() {
                0x7fc0_0000
            } else {
                value.to_bits() as i32
            }
        };
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
        f
    }

    /// Replays `WorldGeneratorFactoryOracle.java` (`.local/terraforged/oracles/world_generator_factory/`),
    /// Engine 0.3.0's real `WorldGeneratorFactory` for four settings variants, one per continent type
    /// (seeds, horizontal scales, volcano weight, levels, control points, filter settings): `SEED` the
    /// context cursor before and after `new WorldGeneratorFactory(createNoCache(s))` and after a direct
    /// `new Heightmap` (equal), and on contexts that drew `pre` times first, the cursor before and after
    /// `context.worldGenerator.get()` (unchanged) and after `new WorldGeneratorFactory(context)`;
    /// `FILTERS`/`CLIMATE`/`GEN` what the factory shares; `POINT` every cell field from the factory's
    /// heightmap, then whether a direct heightmap and the lazy factory's give the same cell; `DRAWN` the
    /// direct factory on the pre-drawn context. `TILE`/`ZOOM`: the first end-to-end tiles, `TileGenerator
    /// .builder().factory(lazy factory).size(f, b).batch(n).build()`'s `generateRegion(rx, rz)` and zoomed
    /// `generateRegion(cx, cz, zoom, filter)` with the real heightmap and filters (erosion, smoothing,
    /// steepness, beach), a fold of every backing cell field and two whole cells.
    #[test]
    fn world_generator_factory_matches_engine_0_3_0() {
        const EXPECTED: &str = "\
SEED 0 MULTI 1234 1251 1251 2 1236 1236 1253
FILTERS 0 MULTI 135 1
CLIMATE 0 MULTI true
GEN 0 MULTI true true
POINT 0 MULTI 00000000 00000000 3e9cbe9b 00000000 00000000 00000000 3f3114ca 3f0f73ca 3ef611d2 3ebc1f57 3e8448e4 3ed9c8bf 674309864963 3f76864c 3f530f9e 3e93f3fc 3f800000 -4522 2316 false coast 4 # true true
DRAWN 0 MULTI 00000000 00000000 3ddd62b6 00000000 00000000 00000000 3f520844 3f133d65 3e1c2432 3d06d4e6 3e8448e4 3ed9c8bf 674309864963 3ecf3b60 3f302d3f 3f330580 3f800000 4909 -4087 false deep_ocean 4
POINT 0 MULTI 41440000 c2c58000 3ee61fcc 00000000 00000000 00000000 3f405523 3f0592db 3ef611d2 3eea3a5a 3e8448e4 3f800000 674309864963 3f0904b4 3e8f2866 3d50a340 3f800000 -4522 2316 false flats-hills 4 # true true
DRAWN 0 MULTI 41440000 c2c58000 3e023b8f 00000000 00000000 00000000 3f6e4f1e 3f1330de 3e1c2432 3cde66de 3e8448e4 3f800000 674309864963 3f3de617 3ccc9980 3f26617b 3f800000 4909 -4087 false deep_ocean 4
POINT 0 MULTI c5800400 46000100 3d9892c5 00000000 00000000 00000000 3f2d29e9 3f12ae96 3ef611d2 00000000 3f6f44c9 3f0bfd8b -18687402696900 3f52d50e 3ee54db4 3da08f1c 3f800000 -4522 2316 false deep_ocean 4 # true true
DRAWN 0 MULTI c5800400 46000100 3e56aa0e 00000000 00000000 00000000 3f149c15 3f139447 3f321320 3e69b69e 3f6f44c9 3f0bfd8b -18687402696900 3f13b0c3 3f43a5f0 3e9e6a88 3f800000 -10920 6333 false ocean 4
POINT 0 MULTI 47c35000 c7c35000 3eebd0e1 00000000 00000000 00000000 3f800000 3f732250 3f400000 3f12c169 3f5b3d2a 3f3466bf 429767312440093 3edcee44 3eefaf1a 3e5af518 3f800000 96576 -102723 false mountain_chain 0 # true true
DRAWN 0 MULTI 47c35000 c7c35000 3eebd0e1 00000000 00000000 00000000 3f721433 3f72b708 3f632a4a 3f4efe68 3f5b3d2a 3f3466bf 429767312440093 3f7c5aa1 3e80f110 3eb1704c 3f800000 97687 -100975 false mountain_chain 0
POINT 0 MULTI cbe4e1c0 44016000 3e539576 00000000 00000000 00000000 3f1b1b89 3f5f96e7 3dd44ed4 3e69230d 3f2a4308 3e29cbd4 -128845600086032018 3eacbd1a 3e740e14 3f04843c 3f800000 -29995418 439 false ocean 1 # true true
DRAWN 0 MULTI cbe4e1c0 44016000 3ec9644d 00000000 00000000 00000000 3e9aa1d9 3f5ca0f3 3ee1c8a6 3f76cf99 3f2a4308 3e29cbd4 -128845600086032018 3d0e6170 3d060270 3f38f5ac 3f800000 -29998614 1676 false mountain_chain 1
TILE 0 MULTI 1 0 0 0 0 fold bb40a70e37961c0e | 3e9c52b5 ba57caba 00000000 3f4e2372 3f3114ca 3f0f73ca 3ef611d2 3ebc1f57 3e8448e4 3ed9c8bf 674309864963 3f76864c 3f530f9e 3e93f3fc 3f800000 -4522 2316 false coast 4 | 3e925dd3 bb46b4fb 00000000 3ea11490 3f3114ca 3f108452 3ef611d2 3ebbbf53 3e8448e4 3e8d5013 674309864963 3f76864c 3f4bdc2a 3e93f3fc 3f800000 -4522 2316 false coast 4
ZOOM 0 MULTI 1 0 0 0 0 fold 4701428ce3099489 | 3e991257 b9de3d68 00000000 3f172caa 3ce1b7d0 3f1b3f6a 3ef611d2 3f3eebf9 3ec9ef7e 3e2476c6 -9676561317208 3f3af398 3e64e9e4 3f74a641 3f26b5d6 -4522 2316 false hills 7 | 3e9a5aec bb9e1967 00000000 3e3c3e8c 3ce1b7d0 3f1acf8d 3ef611d2 3f3f0803 3ec9ef7e 3e4c3209 -9676561317208 3f3af398 3c7d3080 3f74a641 3f27ff9f -4522 2316 false hills 7
SEED 1 SINGLE -987654321 -987654304 -987654304 3 -987654318 -987654318 -987654301
FILTERS 1 SINGLE 17 2
CLIMATE 1 SINGLE true
GEN 1 SINGLE true true
POINT 1 SINGLE 00000000 00000000 3ddd62b6 00000000 00000000 00000000 3f55df19 3f12e6c0 3f1c1301 00000000 3e044f30 3da659e9 -2422361554835 3f4a271e 3cef7360 3f32f641 3f800000 1701 -4963 false deep_ocean 4 # true true
DRAWN 1 SINGLE 00000000 00000000 3e8fd857 00000000 00000000 00000000 3f179dc1 3f120a43 3ec0b800 3f1b2b25 3e044f30 3da659e9 -2422361554835 3f32f641 3f3f6c9b 3ed2e37a 3f800000 3302 -4128 false flats 4
POINT 1 SINGLE 41440000 c2c58000 3de094e0 00000000 00000000 00000000 3f55df19 3f12e6c0 00000000 00000000 3e6fd77e 3d275032 450971566710 3f4a271e 3f4b9c1e 3f32f641 3b92d480 0 0 false deep_ocean 4 # true true
DRAWN 1 SINGLE 41440000 c2c58000 3e9543bf 00000000 00000000 00000000 3ed4500e 3f0bf3db 3ec0b800 3f253e2f 3e6fd77e 3d275032 450971566710 3f272e9c 3deeaf40 3ed2e37a 3f800000 3302 -4128 false badlands-hills 4
POINT 1 SINGLE c5800400 46000100 3d90535b 00000000 00000000 00000000 3f800000 3f0377f0 00000000 00000000 3f21a1fe 3f800000 -19164144066764 3f473caa 3f1e5d7d 3dd25330 3f800000 0 0 false deep_ocean 4 # true true
DRAWN 1 SINGLE c5800400 46000100 3da0961c 00000000 00000000 00000000 3f27b864 3f0bb06e 00000000 00000000 3f21a1fe 3f800000 -19164144066764 3eb99c40 3ed66968 3f3fb581 3f800000 0 0 false deep_ocean 4
POINT 1 SINGLE 47c35000 c7c35000 3dd14a44 00000000 00000000 00000000 3ebae2a6 3f800000 00000000 00000000 3ebc6a32 3f800000 428401512839399 3e7a2188 3ee53fd6 3f6fb8cb 3f800000 0 0 false deep_ocean 1 # true true
DRAWN 1 SINGLE 47c35000 c7c35000 3dddf805 00000000 00000000 00000000 3e21f111 3f800000 00000000 00000000 3ebc6a32 3f800000 428401512839399 3f065d6a 3f36fc64 3f7eb6e6 3f800000 0 0 false deep_ocean 2
POINT 1 SINGLE cbe4e1c0 44016000 3d9055c2 00000000 00000000 00000000 3f57a0b8 3f1c8f31 00000000 00000000 3d070dc8 3e09d567 -128851046104563514 3eda8c31 3ed58a5a 3f143e1d 3f800000 0 0 false deep_ocean 3 # true true
DRAWN 1 SINGLE cbe4e1c0 44016000 3dee6aa2 00000000 00000000 00000000 3f615b75 3f23d859 00000000 00000000 3d070dc8 3e09d567 -128851046104563514 3f72b43a 3f39ac68 3f0b8332 3f800000 0 0 false deep_ocean 3
TILE 1 SINGLE 2 1 0 -1 2 fold 30cfc7d024a8f6d5 | 3db68c48 00000000 00000000 3f800000 3f726c0a 3f189264 3f1c1301 00000000 3e044f30 3f1547dd -2422361554835 3e6f6f32 3f161d6c 3f32f641 3f800000 1701 -4963 false deep_ocean 4 | 3df31547 00000000 00000000 3f800000 3f726c0a 3f189264 3f1c1301 00000000 3e044f30 3eab3ab6 -2422361554835 3e6f6f32 3f5b1d96 3f32f641 3f800000 1701 -4963 false deep_ocean 4
ZOOM 1 SINGLE 2 1 0 0 0 fold dbffff58686b1f22 | 3e016283 00000000 00000000 3f800000 3f800000 3f131b98 00000000 00000000 3e044f30 3f363cfc -2422361554835 3f525c72 3ee1edc8 3c7fa300 3f800000 0 0 false deep_ocean 4 | 3dd38b7a 00000000 00000000 3f800000 3f800000 3f131b98 00000000 00000000 3e044f30 3f800000 -2422361554835 3f525c72 3e3b450c 3c7fa300 3f788c7c 0 0 false deep_ocean 4
SEED 2 MULTI_IMPROVED 591751049 591751069 591751069 4 591751053 591751053 591751073
FILTERS 2 MULTI_IMPROVED 135 1
CLIMATE 2 MULTI_IMPROVED true
GEN 2 MULTI_IMPROVED true true
POINT 2 MULTI_IMPROVED 00000000 00000000 3d9135bc 00000000 00000000 00000000 3f58348b 3f129ac0 3f66f0b6 3d01d729 3f633346 3f800000 343597383934 3e99487c 3d5d4050 3f746c90 3f800000 2181 -6614 false deep_ocean 4 # true true
DRAWN 2 MULTI_IMPROVED 00000000 00000000 3d9135bc 00000000 00000000 00000000 3f655f63 3f1293d5 3f2697a0 00000000 3f633346 3f800000 343597383934 3f698712 3ec28ccc 3ed6a425 3f800000 1198 -6609 false deep_ocean 4
POINT 2 MULTI_IMPROVED 41440000 c2c58000 3d9dc100 00000000 00000000 00000000 3f800000 3f11946a 3f66f0b6 3d4b1cef 3f633346 3ea75211 343597383934 3ed8f061 3ef49c86 3f746c90 3f800000 2181 -6614 false deep_ocean 4 # true true
DRAWN 2 MULTI_IMPROVED 41440000 c2c58000 3daa19a5 00000000 00000000 00000000 3f6ce80f 3f12f025 3f2697a0 3d787302 3f633346 3ea75211 343597383934 3e34dee2 3f7daade 3ed6a425 3f800000 1198 -6609 false deep_ocean 4
POINT 2 MULTI_IMPROVED c5800400 46000100 3da3a164 00000000 00000000 00000000 3f59e71b 3f171bbb 3f56624c 00000000 3f24c6db 3d4e3c4c -17587891068550 3f48696c 3ef7ea64 3f6581df 3f800000 3271 6171 false deep_ocean 4 # true true
DRAWN 2 MULTI_IMPROVED c5800400 46000100 3e12c3ea 00000000 00000000 00000000 3f3994a2 3f19fd54 3f513dec 3e8a6919 3f24c6db 3d4e3c4c -17587891068550 3e995fa6 3f4aeef7 3efe81c2 3f800000 1522 6006 false ocean 3
POINT 2 MULTI_IMPROVED 47c35000 c7c35000 3e561664 00000000 00000000 00000000 3f665fae 3f800000 3b26f680 3f3679d0 3d25e5a0 3f00d0c7 429011398196019 3dee0754 3f059194 3f391480 3f800000 102088 -104853 false plateau-hills 0 # true true
DRAWN 2 MULTI_IMPROVED 47c35000 c7c35000 3dbd952f 00000000 00000000 00000000 3f800000 3f7eafbd 3f0303a8 00000000 3d25e5a0 3f00d0c7 429011398196019 3ddf2a34 3f392662 3d294990 3f800000 98094 -92762 false deep_ocean 0
POINT 2 MULTI_IMPROVED cbe4e1c0 44016000 3e309629 00000000 00000000 00000000 3e39ce65 3f64a1cc 3ec303c7 3ea2a5a3 3e96fc86 3f4874bd -128847343842753764 3f74fc9c 3ecc7860 3ccca290 3f800000 -30006124 5099 false coast 1 # true true
DRAWN 2 MULTI_IMPROVED cbe4e1c0 44016000 3da0fe0b 00000000 00000000 00000000 3e4cf63c 3f73ae3a 3d94186c 00000000 3e96fc86 3f4874bd -128847343842753764 3f571700 3f5b20d6 3e8f86dd 3f800000 -29997736 5435 false deep_ocean 2
TILE 2 MULTI_IMPROVED 1 1 2 3 -2 fold b4f51bbe8a26b8e9 | 3db89dc9 00000000 00000000 3f800000 3f5c28a8 3f1222fc 3f66f0b6 3d5ea571 3f633346 3ebaa592 343597383934 3ebdce35 3ebe0a48 3f746c90 3f800000 2181 -6614 false deep_ocean 4 | 3dcd144f 00000000 00000000 3f800000 3f5c28a8 3f1222fc 3f66f0b6 3d56c784 3f633346 3f0600ba 343597383934 3ebdce35 3efa5628 3f746c90 3f800000 2181 -6614 false deep_ocean 4
ZOOM 2 MULTI_IMPROVED 1 1 2 0 0 fold f4d621a0e079083e | 3d53a763 00000000 00000000 3f800000 3f800000 3eca9dab 3f66f0b6 3de51881 3f0d6825 3f800000 1924145348205 3eee3836 3f5b59ce 3f746c90 3f800000 2181 -6614 false ocean 8 | 3d8d48c2 00000000 00000000 3f800000 3f800000 3ee80969 3f66f0b6 3ddf111b 3f0d6825 3f800000 1924145348205 3e98fa04 3f0cf5d2 3f746c90 3f800000 2181 -6614 false ocean 8
SEED 3 FANCY 7 24 24 5 12 12 29
FILTERS 3 FANCY 0 1
CLIMATE 3 FANCY true
GEN 3 FANCY true true
POINT 3 FANCY 00000000 00000000 3e86e047 00000000 00000000 00000000 3ef0ed92 3f15400d 00000000 3f800000 3f6fd080 3eefb728 1705102016554 3f42d102 3f27326a 3e923354 3dafac70 0 0 false hills 4 # true true
DRAWN 3 FANCY 00000000 00000000 3eaadf5e 00000000 00000000 00000000 3ea13e77 3f0cf638 00000000 3f800000 3f6fd080 3eefb728 1705102016554 3f2b5834 3f3952b2 3f31d83e 3f71f3ce 0 0 false hills 4
POINT 3 FANCY 41440000 c2c58000 3e564b6f 00000000 00000000 00000000 3e2e00d2 3f0a8b2c 00000000 3f800000 3f6fd080 3f35a9d1 1705102016554 3eabaab1 3e8de0da 3e923354 3dfba760 0 0 false lake 5 # true true
DRAWN 3 FANCY 41440000 c2c58000 3eba1215 00000000 00000000 00000000 3ea13e77 3f0b1932 00000000 3f800000 3f6fd080 3f35a9d1 1705102016554 3f2b5834 3f4e6de2 3f31d83e 3f7fe9a6 0 0 false hills 4
POINT 3 FANCY c5800400 46000100 3d72f5ac 00000000 00000000 00000000 3f800000 3e0f616d 00000000 00000000 3e108520 3e8ac7d3 -19408957202156 3dfdf2cc 3f4d7310 3eba02e0 3f800000 0 0 false deep_ocean 9 # true true
DRAWN 3 FANCY c5800400 46000100 3dd92aef 00000000 00000000 00000000 3f800000 3e131f51 00000000 00000000 3e108520 3e8ac7d3 -19408957202156 3f483cf6 3e2a271c 3f137516 3f800000 0 0 false deep_ocean 9
POINT 3 FANCY 47c35000 c7c35000 3e00af1d 00000000 00000000 00000000 3f0a89e6 3f800000 00000000 00000000 3e111492 3f800000 428826714601779 3f13c0a8 3f0b06f2 3d33ed68 3f800000 0 0 false deep_ocean 1 # true true
DRAWN 3 FANCY 47c35000 c7c35000 3e012f14 00000000 00000000 00000000 3f408aff 3f800000 00000000 00000000 3e111492 3f800000 428826714601779 3eb49e09 3f14e2ac 3f5a10bb 3f800000 0 0 false deep_ocean 0
POINT 3 FANCY cbe4e1c0 44016000 3e1e9799 00000000 00000000 00000000 3f3db39e 3f2c5e84 00000000 00000000 3f676dd6 3e95b1be -128845703165247209 3d258af0 3f4d7ee9 3eb52dec 3f800000 0 0 false deep_ocean 3 # true true
DRAWN 3 FANCY cbe4e1c0 44016000 3de69f07 00000000 00000000 00000000 3f4f2621 3f295299 00000000 00000000 3f676dd6 3e95b1be -128845703165247209 3eb537c4 3deb1428 3ea53ce0 3f800000 0 0 false deep_ocean 3
TILE 3 FANCY 0 2 0 -5 -7 fold cbc71257fc7b63b9 | 3e8031bd 00000000 00000000 3e29d9cc 3e99339b 3f124ef6 00000000 3f800000 3f6fd080 3d2e0747 1705102016554 3e0907f2 3f3370a1 3e923354 3b0208df 0 0 false hills 5 | 3e6f1341 00000000 00000000 3f800000 3e99339b 3f1293d5 00000000 3f800000 3f6fd080 3e0f60a0 1705102016554 3e0907f2 3f252187 3e923354 3d3ab0f0 0 0 false lake 5
ZOOM 3 FANCY 0 2 0 0 0 fold b2db9103551c1464 | 3e5bffa8 00000000 00000000 3f800000 3d1c39bc 3ee237ff 00000000 3e7fa999 3ecc461c 3f1be1dd 8486855375989 3f7496e5 3e2e1070 3f36e656 3f800000 0 0 false ocean 6 | 3ea1f136 00000000 00000000 3f800000 3dd7079a 3eecccbe 00000000 3ed51637 3ec19ebb 3d05d835 5712306503856 3cf5b130 3e9f4c64 3e3b05ec 3f800000 0 0 false coast 6
";
        use super::super::terraforged_context::GeneratorSettings;
        use super::super::terraforged_engine::ContinentType;
        let bits = |v: f32| format!("{:08x}", v.to_bits());
        /// `String.hashCode` of an ASCII name.
        fn hash(name: &str) -> i32 {
            name.bytes()
                .fold(0_i32, |h, b| h.wrapping_mul(31).wrapping_add(i32::from(b)))
        }
        let floats = |c: &Cell| {
            [
                c.value,
                c.erosion,
                c.sediment,
                c.gradient,
                c.moisture,
                c.temperature,
                c.continent_id,
                c.continent_edge,
                c.terrain_region_id,
                c.terrain_region_edge,
                c.biome_region_id,
                c.biome_region_edge,
                c.macro_biome_id,
                c.river_mask,
            ]
        };
        let line = |c: &Cell| {
            let f = floats(c).map(|v| bits(if v.is_nan() { f32::NAN } else { v }));
            format!(
                "{} {} {} {} {} {} {} {}",
                f[..10].join(" "),
                c.terrain_region_center,
                f[10..].join(" "),
                c.continent_x,
                c.continent_z,
                c.erosion_mask,
                c.terrain.name(),
                c.biome as i32,
            )
        };
        let fold_all = |cell: &Cell, f: i64| {
            let mut f = floats(cell).into_iter().fold(f, |f, v| {
                fold(
                    f,
                    if v.is_nan() {
                        0x7fc0_0000
                    } else {
                        v.to_bits() as i32
                    },
                )
            });
            let center = cell.terrain_region_center;
            for v in [
                center as i32,
                (center as u64 >> 32) as i32,
                cell.continent_x,
                cell.continent_z,
                i32::from(cell.erosion_mask),
                hash(cell.terrain.name()),
                cell.biome as i32,
            ] {
                f = fold(f, v);
            }
            f
        };
        let tile_line = |tag: String, tile: &Tile| {
            let total = tile.block_size().total;
            let at = |i: i32| tile.raw_cell(i % total, i / total).expect("backing cell");
            let f = (0..total * total).fold(17, |f, i| fold_all(at(i), f));
            format!(
                "{tag} {} {} fold {f:016x} | {} | {}\n",
                tile.region_x(),
                tile.region_z(),
                line(at(0)),
                line(at(total * total / 2 + 7))
            )
        };
        let point = |map: &Heightmap, terrains: &TerrainRegistry, x: f32, z: f32| {
            let mut cell = Cell::new(terrains);
            map.apply(&mut cell, x, z);
            line(&cell)
        };
        let points = [
            (0.0, 0.0),
            (12.25, -98.75),
            (-4096.5, 8192.25),
            (100_000.0, -100_000.0),
            (-3.0e7, 517.5),
        ];
        let tiles = [
            (1, 0, 0, 0, 0),
            (2, 1, -1, 2, 0),
            (1, 1, 3, -2, 2),
            (0, 2, -5, -7, 0),
        ];
        let names = ["MULTI", "SINGLE", "MULTI_IMPROVED", "FANCY"];
        let mut out = String::new();
        for variant in 0..4 {
            let mut s = GeneratorSettings::default();
            s.world.continent.continent_type = ContinentType::VALUES[variant];
            match variant {
                0 => s.world.seed = 1234,
                1 => {
                    s.world.seed = -987_654_321;
                    s.terrain.general.global_horizontal_scale = 2.5;
                    s.filters.erosion.droplets_per_chunk = 17;
                    s.filters.smoothing.iterations = 2;
                }
                2 => {
                    s.world.seed = 0x1_2345_6789;
                    s.terrain.general.global_horizontal_scale = 0.37;
                    s.terrain.volcano.weight = 20.0;
                    s.world.properties.world_height = 384;
                    s.world.properties.sea_level = 62;
                    s.world.control_points.inland = 0.6;
                }
                _ => {
                    s.world.seed = 7;
                    s.filters.erosion.droplets_per_chunk = 0;
                    s.filters.smoothing.smoothing_radius = 3.5;
                }
            }
            let tag = format!("{variant} {}", names[variant]);
            let mut direct = GeneratorContext::create_no_cache(s);
            let start = direct.seed.get();
            let factory = WorldGeneratorFactory::new(&mut direct);
            let mut plain = GeneratorContext::create_no_cache(s);
            let heightmap = Heightmap::new(&mut plain);
            let pre = variant as i32 + 2;
            let (mut lazy_context, mut drawn) =
                (GeneratorContext::new(s), GeneratorContext::new(s));
            for _ in 0..pre {
                lazy_context.seed.next();
                drawn.seed.next();
            }
            let lazy_before = lazy_context.seed.get();
            let lazy = lazy_context.world_generator();
            let drawn_factory = WorldGeneratorFactory::new(&mut drawn);
            out += &format!(
                "SEED {tag} {start} {} {} {pre} {lazy_before} {} {}\n",
                direct.seed.get(),
                plain.seed.get(),
                lazy_context.seed.get(),
                drawn.seed.get()
            );
            // The factory's registry is the context's, the one its heightmap registered into.
            assert!(Arc::ptr_eq(factory.terrains(), &direct.terrain));
            assert!(Arc::ptr_eq(lazy.terrains(), &lazy_context.terrain));
            let settings = factory.filters().settings();
            out += &format!(
                "FILTERS {tag} {} {}\n",
                settings.erosion.droplets_per_chunk, settings.smoothing.iterations
            );
            let same_climate = std::ptr::eq(factory.climate(), factory.heightmap().climate());
            out += &format!("CLIMATE {tag} {same_climate}\n");
            let (first, second) = (factory.get(), factory.get());
            out += &format!(
                "GEN {tag} {} {}\n",
                Arc::ptr_eq(first.heightmap(), second.heightmap()),
                Arc::ptr_eq(first.filters(), factory.filters())
            );
            for (x, z) in points {
                let got = point(factory.heightmap(), factory.terrains(), x, z);
                out += &format!(
                    "POINT {tag} {} {} {got} # {} {}\n",
                    bits(x),
                    bits(z),
                    point(&heightmap, &plain.terrain, x, z) == got,
                    point(lazy.heightmap(), lazy.terrains(), x, z) == got
                );
                out += &format!(
                    "DRAWN {tag} {} {} {}\n",
                    bits(x),
                    bits(z),
                    point(drawn_factory.heightmap(), drawn_factory.terrains(), x, z)
                );
            }
            let (factor, border, rx, rz, batch) = tiles[variant];
            let generator = TileGenerator::builder()
                .world_factory(&lazy)
                .size(factor, border)
                .batch(batch)
                .build();
            let label = format!("{tag} {factor} {border} {batch}");
            out += &tile_line(format!("TILE {label}"), &generator.generate_region(rx, rz));
            let v = variant as f32;
            let zoomed = generator.generate_zoomed_region(
                -1500.5 + 977.0 * v,
                250.25 - 311.0 * v,
                1.0 + 1.5 * v,
                variant % 2 == 0,
            );
            out += &tile_line(format!("ZOOM {label}"), &zoomed);
        }
        for (got, want) in out.lines().zip(EXPECTED.lines()) {
            assert_eq!(got, want);
        }
        assert_eq!(out.lines().count(), EXPECTED.lines().count());
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
    /// `TileGeneratorStriped` (`batch(n)`, on a four-thread pool) for batch counts 1, 3, 2, 7 and
    /// 100 (zoomed: 3 and 100), with the lookup count and an order-free sum of the stage calls
    /// (their cells equal the `g`/`z` lines'); `e` lines (first run) counts 0 and -2, where Engine
    /// throws; `u` and `w` lines (runs 0, 2 and 3) generators from Engine's real builder,
    /// `batch(n)` unset, 0, -3, 1 or 4, with and without `striped()` and a pool: the class `build()`
    /// picks, its `generateRegion`, its lazy `getTile` and its zoomed `generateRegion` (batched
    /// for any positive count, 1 included; without a pool the batches run on the calling thread,
    /// so the stage trace is Engine's too); `s` lines `chunkToRegion` for shift factors Java masks
    /// to five bits; `d` lines the builder's defaults and overrides, and `n` `build()` without a
    /// factory, which throws.
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
                    if count != 3 && count != 100 {
                        continue;
                    }
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
            for striped in [false, true].into_iter().filter(|_| run == 0) {
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
            // The builder: each batch count (unset, 0, -3, 1, 4), striped or not, with or without
            // a pool; the trace is Engine's only on the calling thread (no pool).
            let mut combo = 0;
            let batches: &[Option<i32>] = if matches!(run, 0 | 2 | 3) {
                &[None, Some(0), Some(-3), Some(1), Some(4)]
            } else {
                &[]
            };
            for &batch in batches {
                for striped in [false, true] {
                    for pool in [false, true] {
                        let filters = WorldFilters::new(
                            seed,
                            settings,
                            Levels::new(256, 63),
                            points,
                            &terrains,
                        );
                        let mut builder = TileGenerator::builder().size(factor, border).factory(
                            Arc::clone(&terrains),
                            WorldGenerator::new(FillHeightmap::new(kinds.clone()), filters),
                        );
                        if let Some(batch) = batch {
                            builder = builder.batch(batch);
                        }
                        if striped {
                            builder = builder.striped();
                        }
                        if pool {
                            builder = builder.pool(4);
                        }
                        let built = builder.build();
                        let heightmap = built.generator().heightmap();
                        let head = format!(
                            "{run} {} {} {}",
                            batch.map_or("-".into(), |batch| batch.to_string()),
                            u8::from(striped),
                            u8::from(pool)
                        );
                        let trace = |(lookups, trace): (i32, i64), sum: i64| {
                            let trace = if pool {
                                "-".into()
                            } else {
                                format!("{trace:x}")
                            };
                            format!("{lookups} {trace} {sum:x}")
                        };
                        let take = || {
                            let sum = heightmap.sum.load(Ordering::SeqCst);
                            trace(heightmap.take(), sum)
                        };
                        let tile = built.generate_region(region_x, region_z);
                        let stages = take();
                        let class = match built.kind {
                            Kind::Plain => "TileGenerator",
                            Kind::Batched => "TileGeneratorBatched",
                            Kind::Striped => "TileGeneratorStriped",
                        };
                        let lazy = fold_tile(built.get_tile(region_x, region_z).get());
                        heightmap.take();
                        let cells = fold_tile(&tile);
                        out += &format!("u {head} {class} {stages} {cells:x} {lazy:x}\n");
                        let set = (run + combo) % zooms.len();
                        let (center_x, center_z, zoom, filter) = zooms[set];
                        let tile = built.generate_zoomed_region(center_x, center_z, zoom, filter);
                        let stages = take();
                        out += &format!("w {head} {set} {stages} {:x}\n", fold_tile(&tile));
                        combo += 1;
                    }
                }
            }
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
        // The builder's defaults (factor 0, border 0, plain), the last call winning, `striped()` and
        // a pool alone keeping the plain generator, and `build()` without a factory.
        for case in 0..3 {
            let filters = WorldFilters::new(
                77,
                FilterSettings::default(),
                Levels::new(256, 63),
                WorldControlPoints::default(),
                &terrains,
            );
            let heightmap = FillHeightmap::new(kinds.clone());
            let mut builder = TileGenerator::builder().factory(
                Arc::clone(&terrains),
                WorldGenerator::new(heightmap, filters),
            );
            if case == 1 {
                builder = builder
                    .size(3, 2)
                    .factor(1)
                    .border(0)
                    .batch(5)
                    .batch(0)
                    .striped();
            } else if case == 2 {
                builder = builder.batch(-1).striped().pool(4);
            }
            let built = builder.build();
            assert_eq!(built.kind, Kind::Plain);
            let tile = built.generate_region(-1, 2);
            let (lookups, trace) = built.generator().heightmap().take();
            let (chunks, blocks) = (tile.chunk_size(), tile.block_size());
            out += &format!(
                "d {case} TileGenerator {} {} {} {} {} {lookups} {trace:x} {:x}\n",
                chunks.total,
                blocks.size,
                blocks.total,
                tile.region_x(),
                tile.region_z(),
                fold_tile(&tile)
            );
        }
        let unbuilt = std::panic::catch_unwind(|| {
            TileGenerator::<FillHeightmap, NoFilters>::builder()
                .size(1, 1)
                .batch(2)
                .build()
        });
        assert!(unbuilt.is_err());
        out += "n throws NullPointerException\n";
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
    /// odd, uneven and huge counts included (batches past the last chunk are skipped, not built),
    /// and a stage's panic reaches the caller with its payload.
    #[test]
    fn batches_match_any_thread_count_and_propagate_panics() {
        let terrains = Arc::new(TerrainRegistry::new());
        let kinds = ["none", "coast", "beach", "hills"]
            .map(|name| terrains.get_by_name(name).expect("built-in terrain"));
        let heightmap = FillHeightmap::new(kinds);
        for (count, workers) in [(1, 8), (3, 1), (3, 8), (5, 3), (2, 64), (i32::MAX, 4)] {
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

    /// The builder's pool: `ThreadPools.create(size)` has `max(2, size)` batch threads, no pool
    /// runs the batches on the calling thread, and [`TileGenerator::new`] uses the default pool.
    #[test]
    fn builder_pools_set_the_batch_threads() {
        let terrains = Arc::new(TerrainRegistry::new());
        let kinds = ["none", "coast", "beach", "hills"]
            .map(|name| terrains.get_by_name(name).expect("built-in terrain"));
        let create = || WorldGenerator::new(FillHeightmap::new(kinds.clone()), NoFilters);
        let built = |pool: Option<i32>| {
            let mut builder = TileGenerator::builder()
                .batch(2)
                .factory(Arc::clone(&terrains), create());
            if let Some(size) = pool {
                builder = builder.pool(size);
            }
            builder.build().batch_threads
        };
        assert_eq!(built(None), Some(1));
        for (size, threads) in [(-5, 2), (0, 2), (1, 2), (2, 2), (8, 8)] {
            assert_eq!(built(Some(size)), Some(threads));
        }
        let plain = TileGenerator::new(0, 0, 2, Arc::clone(&terrains), create());
        assert_eq!((plain.kind, plain.batch_threads), (Kind::Plain, None));
    }

    /// Engine shares one tile generator, its tile cache and the chunk readers it hands out between
    /// its generation threads.
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
        shareable::<TileCache<SharedHeightmap, WorldFilters>>();
        shareable::<CachedChunk>();
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

    /// The clock of [`tile_cache_matches_engine_oracle`]'s caches and entries.
    static TILE_CACHE_CLOCK: AtomicI64 = AtomicI64::new(0);

    fn tile_cache_clock() -> i64 {
        TILE_CACHE_CLOCK.load(Ordering::SeqCst)
    }

    fn set_clock(now: i64) {
        TILE_CACHE_CLOCK.store(now, Ordering::SeqCst);
    }

    /// Eight threads `getTile(regionX, regionZ)` at once: `<all the same tile> <lookups> <stage
    /// sum> <cells>` (the oracle's `concurrent`).
    fn concurrent_tiles<F>(
        cache: &TileCache<FillHeightmap, F>,
        region_x: i32,
        region_z: i32,
    ) -> String
    where
        F: TileFilters + Send + Sync + 'static,
    {
        let heightmap = cache.generator.generator().heightmap();
        heightmap.take_sum();
        let barrier = std::sync::Barrier::new(8);
        let tiles: Vec<_> = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..8)
                .map(|_| {
                    scope.spawn(|| {
                        barrier.wait();
                        cache.get_tile_at(region_x, region_z)
                    })
                })
                .collect();
            handles.into_iter().map(|h| h.join().unwrap()).collect()
        });
        let same = tiles.iter().all(|tile| Arc::ptr_eq(tile, &tiles[0]));
        let (lookups, sum) = heightmap.take_sum();
        format!("{same} {lookups} {sum:x} {:x}", fold_tile(tiles[0].call()))
    }

    /// Replays `TileCacheOracle.java` (`.local/terraforged/oracles/tile_cache/`): Engine 0.3.0's
    /// real `TileCache` from `TileGenerator.cached()` over `TileGeneratorOracle`'s stub heightmap
    /// and filters. `p` lines: which pools queue (`size() > 4`); `k` lines: the region ids;
    /// `n` lines (pool `NONE`, the tile generator oracle's run 0): nothing is queued, `getTile`
    /// generates the tile bit for bit as `generateRegion` (its `g 0` line), hits run nothing and
    /// return the same tile, `get` does not stamp while `getTile`/`getTileIfPresent` do,
    /// `getOrCompute` runs nothing and `getTileIfPresent` then runs the done-at-once entry, the
    /// sweep keeps an entry stamped exactly a lifetime ago and drops one a millisecond older (the
    /// next `getTile` makes a new tile), and eight threads asking for one region generate it once;
    /// `q` lines (`ThreadPools.create(8)`, run 3): `queueRegion`/`queueChunk` start tiles, eight
    /// threads share one tile, and a batched generator's cache; `h` lines: `getChunk` (region,
    /// chunk-array slot, position, cells and `visit` rectangles of negative, extreme and aliasing
    /// chunks), a reader for every chunk around two regions, `getChunk` stamping while reads do
    /// not, a reader outliving its swept entry, and eight threads asking for one chunk. The oracle
    /// injects stamps through Engine's private fields; here the cache's clock sets them.
    #[test]
    fn tile_cache_matches_engine_oracle() {
        let terrains = Arc::new(TerrainRegistry::new());
        let kinds = ["none", "coast", "beach", "hills"]
            .map(|name| terrains.get_by_name(name).expect("built-in terrain"));
        let filters = |seed, droplets, passes, beach| {
            let mut settings = FilterSettings::default();
            settings.erosion.droplets_per_chunk = droplets;
            settings.smoothing.iterations = passes;
            let points = WorldControlPoints {
                beach,
                ..WorldControlPoints::default()
            };
            WorldFilters::new(seed, settings, Levels::new(256, 63), points, &terrains)
        };
        let world = |group: usize| {
            let filters = if group == 0 {
                filters(12345, 20, 1, 0.327)
            } else {
                filters(-9, 5, 2, 0.4)
            };
            (
                Arc::clone(&terrains),
                WorldGenerator::new(FillHeightmap::new(kinds.clone()), filters),
            )
        };
        let measured = |heightmap: &FillHeightmap, tile: &Tile| {
            let (lookups, trace) = heightmap.take();
            let (rx, rz) = (tile.region_x(), tile.region_z());
            format!("{lookups} {trace:x} {rx} {rz} {:x}", fold_tile(tile))
        };
        set_clock(0);
        let (registry, generator) = world(0);
        let none = Arc::new(
            TileGenerator::builder()
                .size(1, 1)
                .factory(registry, generator)
                .build(),
        );
        let heightmap = none.generator().heightmap();
        let cache = TileCache::with_clock(Arc::clone(&none), none.pool.clone(), tile_cache_clock);
        let mut out = format!("c {} 256 60000\n", cache.cache.name());
        out += &format!("p - {} {}\n", ThreadPool::NONE.size(), cache.can_queue);
        for size in 1..=9 {
            let pool = ThreadPool::create(size);
            let queues = TileCache::new(Arc::clone(&none), pool.clone()).can_queue;
            out += &format!("p {size} {} {queues}\n", pool.size());
        }
        let regions = [
            (-1, 2),
            (5, -6),
            (0, 0),
            (-7, 9),
            (3, -4),
            (i32::MIN, i32::MAX),
            (-1, -1),
        ];
        for (rx, rz) in regions {
            let id = Tile::region_id_for(rx, rz);
            let (x, z) = (Tile::region_x_from(id), Tile::region_z_from(id));
            out += &format!("k {rx} {rz} {id} {x} {z}\n");
        }
        let (id, other) = (Tile::region_id_for(-1, 2), Tile::region_id_for(5, -6));
        heightmap.take();
        cache.queue_region(-1, 2);
        cache.queue_chunk(-2, 4);
        let absent = (
            cache.get(id).is_none(),
            cache.get_tile_if_present(id).is_none(),
        );
        out += &format!("n0 {} {} {}\n", absent.0, absent.1, heightmap.take().0);
        let first = cache.get_tile_at(-1, 2);
        out += &format!("n1 {}\n", measured(heightmap, first.call()));
        let entry = cache.get(id).unwrap();
        set_clock(5);
        entry.get();
        set_clock(6);
        cache.get(id);
        let after_get = entry.timestamp();
        let same = Arc::ptr_eq(&cache.get_tile(id), &first);
        let after_tile = entry.timestamp();
        set_clock(5);
        entry.get();
        set_clock(7);
        let present = cache
            .get_tile_if_present_at(-1, 2)
            .is_some_and(|tile| Arc::ptr_eq(&tile, &first));
        let (lookups, _) = heightmap.take();
        let recomputed = Arc::ptr_eq(&cache.get_or_compute(id), &entry);
        out += &format!(
            "n2 {same} {present} {lookups} {after_get} {} {} {recomputed}\n",
            after_tile != 5,
            entry.timestamp() != 5
        );
        let computed = cache.get_or_compute(other);
        let cached = cache.get(other).is_some_and(|e| Arc::ptr_eq(&e, &computed));
        let before = format!("{cached} {} {}", computed.is_done(), heightmap.take().0);
        let present = cache.get_tile_if_present(other).expect("done at once");
        let line = measured(heightmap, present.call());
        let again = Arc::ptr_eq(&cache.get_tile(other), &present);
        out += &format!("n3 {before} {line} {again}\n");
        set_clock(1000);
        cache.get(id).unwrap().get();
        set_clock(1001);
        cache.get(other).unwrap().get();
        set_clock(61_000);
        let kept = cache.cache.run();
        set_clock(61_001);
        let swept = cache.cache.run();
        let gone = (cache.get(id).is_none(), cache.get(other).is_some());
        let again = cache.get_tile_at(-1, 2);
        let line = measured(heightmap, again.call());
        let new = !Arc::ptr_eq(&again, &first);
        out += &format!("n4 {kept} {swept} {} {} {new} {line}\n", gone.0, gone.1);
        set_clock(i64::MAX);
        let all = cache.cache.run();
        let gone = (cache.get(id).is_none(), cache.get(other).is_none());
        let line = concurrent_tiles(&cache, 7, -3);
        out += &format!("n5 {all} {} {} {line}\n", gone.0, gone.1);
        set_clock(0);
        let (registry, generator) = world(1);
        let pooled = TileGenerator::builder()
            .size(0, 2)
            .pool(8)
            .factory(registry, generator);
        let pc = Arc::new(pooled.build()).cached();
        let heightmap = pc.generator.generator().heightmap();
        heightmap.take();
        pc.queue_region(-7, 9);
        let queued = pc.get(Tile::region_id_for(-7, 9)).is_some();
        let tile = pc.get_tile_at(-7, 9);
        let line = measured(heightmap, tile.call());
        let present = pc
            .get_tile_if_present_at(-7, 9)
            .is_some_and(|e| Arc::ptr_eq(&e, &tile));
        out += &format!("q0 {} {queued} {line} {present}\n", pc.can_queue);
        pc.queue_chunk(3, -4);
        let queued = pc.get(Tile::region_id_for(3, -4)).is_some();
        let line = measured(heightmap, pc.get_tile_at(3, -4).call());
        out += &format!("q1 {queued} {line}\n");
        out += &format!("q2 {}\n", concurrent_tiles(&pc, 10, 11));
        let (registry, generator) = world(0);
        let batched = TileGenerator::builder().size(1, 1).batch(3).pool(8);
        let batched = Arc::new(batched.factory(registry, generator).build());
        assert_eq!(batched.kind, Kind::Batched);
        let bc = Arc::clone(&batched).cached();
        let tile = bc.get_tile_at(-1, 2);
        let (lookups, sum) = batched.generator().heightmap().take_sum();
        let cells = fold_tile(tile.call());
        out += &format!("q3 TileGeneratorBatched {lookups} {sum:x} {cells:x}\n");
        let hc = TileCache::with_clock(Arc::clone(&none), ThreadPool::NONE, tile_cache_clock);
        let chunks = [
            (-3, 5),
            (0, 0),
            (-1, -1),
            (3, -4),
            (5, -11),
            (-2, 4),
            (-4, 4),
            (7, 7),
            (-9, -9),
            (i32::MIN, i32::MAX),
            (i32::MAX, i32::MIN),
            (-100_001, 77_777),
        ];
        for (cx, cz) in chunks {
            out += &format!("h {}\n", chunk_line(&hc, cx, cz));
        }
        let (a, b) = (readable(&hc, -2, 2), readable(&pc, -7, 9));
        out += &format!("h0 {a} {b}\n");
        let entry = hc.get(Tile::region_id_for(-2, 2)).unwrap();
        set_clock(5);
        entry.get();
        set_clock(6);
        let reader = hc.get_chunk(-3, 5).unwrap();
        let stamped = entry.timestamp() != 5;
        set_clock(5);
        entry.get();
        set_clock(7);
        let cells = read_cells(&reader);
        reader.reader().cell(3, 4);
        visit(&reader, [0, 0, 15, 15]);
        let unstamped = entry.timestamp() == 5;
        set_clock(i64::MAX);
        let swept = hc.cache.run();
        let gone = hc.get(Tile::region_id_for(-2, 2)).is_none();
        let kept = read_cells(&reader) == cells;
        set_clock(0);
        none.generator().heightmap().take();
        let fresh = hc.get_chunk(-3, 5).unwrap();
        let new = !std::ptr::eq(fresh.tile(), reader.tile());
        let new_cell = !std::ptr::eq(fresh.reader().cell(0, 0), reader.reader().cell(0, 0));
        let lookups = none.generator().heightmap().take().0;
        out += &format!(
            "h1 {stamped} {unstamped} {swept} {gone} {kept} {new} {new_cell} {lookups} {}\n",
            read_cells(&fresh)
        );
        out += &format!("h2 {}\n", chunk_line(&pc, -6, 13));
        out += &format!("h3 {}\n", concurrent_chunks(&pc, -20, 31));
        assert_eq!(out, TILE_CACHE_ORACLE);
    }

    /// `chunkLine`: `<cx> <cz> <rx> <rz> <lookups> <slot> <chunkX> <chunkZ> <blockX> <blockZ> <cells>
    /// <visits...>` of `getChunk(cx, cz)`; the slot is the chunk array index the reader was made at.
    fn chunk_line(cache: &TileCache<FillHeightmap, WorldFilters>, cx: i32, cz: i32) -> String {
        cache.generator.generator().heightmap().take();
        let reader = cache
            .get_chunk(cx, cz)
            .expect("a generated tile fills every chunk");
        let (rx, rz) = (cache.chunk_to_region(cx), cache.chunk_to_region(cz));
        let lookups = cache.generator.generator().heightmap().take().0;
        let (tile, chunk) = (reader.tile(), reader.chunk());
        let (total, border) = (tile.chunk_size().total, tile.offset_chunks());
        let slot_x = chunk
            .chunk_x()
            .wrapping_sub(tile.chunk_x())
            .wrapping_add(border);
        let slot_z = chunk
            .chunk_z()
            .wrapping_sub(tile.chunk_z())
            .wrapping_add(border);
        let slot = slot_z.wrapping_mul(total).wrapping_add(slot_x);
        assert_eq!((tile.region_x(), tile.region_z()), (rx, rz));
        let mut line = format!(
            "{cx} {cz} {rx} {rz} {lookups} {slot} {} {} {} {} {}",
            chunk.chunk_x(),
            chunk.chunk_z(),
            chunk.block_x(),
            chunk.block_z(),
            read_cells(&reader)
        );
        for rect in [
            [-3, 2, 20, 9],
            [4, 4, 9, 12],
            [0, 0, 15, 15],
            [10, -5, 30, 15],
            [2, 1, 8, 15],
            [-40, -40, -1, 30],
        ] {
            line += &format!(" {}", visit(&reader, rect));
        }
        line
    }

    /// `readCells`: the reader's `iterate`, each cell with its `dx`, `dz`.
    fn read_cells(chunk: &CachedChunk) -> String {
        let mut f = 17;
        chunk.reader().iterate(|cell, dx, dz| {
            f = fold(fold(fold_cell(f, cell), dx), dz);
        });
        format!("{f:x}")
    }

    /// `visit`: `<count>:<fold>` of a rectangle relative to the chunk's block position.
    fn visit(chunk: &CachedChunk, [a, b, c, d]: [i32; 4]) -> String {
        let (mut f, mut n) = (17, 0);
        let (bx, bz) = (chunk.chunk().block_x(), chunk.chunk().block_z());
        let (min_x, min_z) = (bx.wrapping_add(a), bz.wrapping_add(b));
        let (max_x, max_z) = (bx.wrapping_add(c), bz.wrapping_add(d));
        chunk
            .reader()
            .visit(min_x, min_z, max_x, max_z, |cell, x, z| {
                n += 1;
                f = fold(fold(fold_cell(f, cell), x), z);
            });
        format!("{n}:{f:x}")
    }

    /// `readable`: `<found>/<all>` readers of `getChunk` over 8x8 chunks around a region's first.
    fn readable(cache: &TileCache<FillHeightmap, WorldFilters>, rx: i32, rz: i32) -> String {
        let mut size = 1;
        while cache.chunk_to_region(size) == 0 {
            size += 1;
        }
        let (mut found, mut all) = (0, 0);
        for cz in rz * size - 4..rz * size + 4 {
            for cx in rx * size - 4..rx * size + 4 {
                all += 1;
                found += i32::from(cache.get_chunk(cx, cz).is_some());
            }
        }
        format!("{found}/{all}")
    }

    /// `concurrentChunks`: eight threads `getChunk(cx, cz)` at once: `<all the same reader>
    /// <lookups> <stage sum> <cells>`.
    fn concurrent_chunks(
        cache: &TileCache<FillHeightmap, WorldFilters>,
        cx: i32,
        cz: i32,
    ) -> String {
        let heightmap = cache.generator.generator().heightmap();
        heightmap.take_sum();
        let barrier = std::sync::Barrier::new(8);
        let chunks: Vec<_> = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..8)
                .map(|_| {
                    scope.spawn(|| {
                        barrier.wait();
                        cache.get_chunk(cx, cz).unwrap()
                    })
                })
                .collect();
            handles.into_iter().map(|h| h.join().unwrap()).collect()
        });
        let position = |c: &CachedChunk| (c.chunk().chunk_x(), c.chunk().chunk_z());
        let same = chunks
            .iter()
            .all(|c| Arc::ptr_eq(&c.tile, &chunks[0].tile) && position(c) == position(&chunks[0]));
        let (lookups, sum) = heightmap.take_sum();
        format!("{same} {lookups} {sum:x} {}", read_cells(&chunks[0]))
    }

    /// The cache Engine's `TileCache` builds: 256 entries (the oldest insertion evicted first) and a
    /// sweep every 20 seconds, run by the next `getOrCompute` once due; tiles are generated only by
    /// `get`, never by `getOrCompute` on `NONE`.
    #[test]
    fn tile_cache_bounds_and_sweep_interval() {
        static CLOCK: AtomicI64 = AtomicI64::new(0);
        fn clock() -> i64 {
            CLOCK.load(Ordering::SeqCst)
        }
        let terrains = Arc::new(TerrainRegistry::new());
        let kinds = ["none", "coast", "beach", "hills"]
            .map(|name| terrains.get_by_name(name).expect("built-in terrain"));
        let generator = WorldGenerator::new(FillHeightmap::new(kinds), NoFilters);
        let generator = Arc::new(TileGenerator::new(0, 0, 0, terrains, generator));
        let cache = TileCache::with_clock(Arc::clone(&generator), ThreadPool::NONE, clock);
        for x in 0..=256 {
            cache.get_or_compute(Tile::region_id_for(x, 0));
        }
        assert!(cache.get(Tile::region_id_for(0, 0)).is_none());
        assert!(cache.get(Tile::region_id_for(1, 0)).is_some());
        assert_eq!(generator.generator().heightmap().take(), (0, 17));
        let id = |x| Tile::region_id_for(x, 0);
        CLOCK.store(60_001, Ordering::SeqCst);
        let swept = cache.get_or_compute(id(-1));
        assert!(
            cache.get(id(1)).is_none(),
            "stamped at 0, swept at 60.001 s"
        );
        CLOCK.store(10_000, Ordering::SeqCst);
        swept.get();
        CLOCK.store(79_999, Ordering::SeqCst);
        cache.get_or_compute(id(-2));
        assert!(
            cache.get(id(-1)).is_some(),
            "expired, but no sweep within 20 s"
        );
        CLOCK.store(80_001, Ordering::SeqCst);
        cache.get_or_compute(id(-2));
        assert!(cache.get(id(-1)).is_none(), "the next sweep drops it");
        assert!(cache.get(id(-2)).is_some());
    }

    /// `TileCacheOracle.java`'s output (`./run.sh` writes it to `out.txt`).
    const TILE_CACHE_ORACLE: &str = "\
c TileCache 256 60000
p - 0 false
p 1 3 false
p 2 3 false
p 3 4 false
p 4 6 true
p 5 7 true
p 6 9 true
p 7 10 true
p 8 12 true
p 9 13 true
k -1 2 -4294967294 -1 2
k 5 -6 25769803770 5 -6
k 0 0 0 0 0
k -7 9 -30064771063 -7 9
k 3 -4 17179869180 3 -4
k -2147483648 2147483647 -9223372034707292161 -2147483648 2147483647
k -1 -1 -1 -1 -1
n0 true true 0
n1 132 933dedba501bf8f1 -1 2 2afe9d3c12774c0d
n2 true true 0 5 true true true
n3 true true 0 137 7e3196318cb259b8 5 -6 bdd7e0198d0c33c3 true
n4 0 1 true true true 132 933dedba501bf8f1 -1 2 2afe9d3c12774c0d
n5 2 true true true 256 1dd0a6c58cacb00 13fed9e732db2fce
q0 true true 326 e3c4cdf7c5eefaee -7 9 6f363908c2b37e18 true
q1 true 326 aeeb33bba750cb4e 3 -4 3fbc3560bca57054
q2 true 176 31575308f8b822c 456ad7f1ef76153
q3 TileGeneratorBatched 140 ff293731a98fd032 2afe9d3c12774c0d
h -3 5 -2 2 256 10 -3 5 -48 80 1c76c64b231b9e88 0:11 0:11 0:11 0:11 0:11 0:11
h 0 0 0 0 137 5 0 0 0 0 1d7a4fc94372f4f3 140:c2dbedd345e8183d 54:d6d30be6fd75be10 256:1d7a4fc94372f4f3 96:ee4e876d7c3113ca 112:ecea6c7669397c65 0:11
h -1 -1 -1 -1 137 10 -1 -1 -16 -16 6be14b265241aadb 140:c5a5ec4b81a05cb2 54:be1b3ad93938522d 256:495ddf5e1c7faadb 96:cb50889885577e47 112:ddaae38a911726b6 0:11
h 3 -4 1 -2 260 6 3 -4 48 -64 52db9415eb863fe 0:11 0:11 0:11 0:11 0:11 0:11
h 5 -11 2 -6 137 10 5 -11 80 -176 a46094caeb856d86 0:11 0:11 0:11 0:11 0:11 0:11
h -2 4 -1 2 132 5 -2 4 -32 64 12d93c1bc65897e3 0:11 0:11 0:11 0:11 0:11 0:11
h -4 4 -2 2 0 5 -4 4 -64 64 2ca690240f8d6136 0:11 0:11 0:11 0:11 0:11 0:11
h 7 7 3 3 137 10 7 7 112 112 bd945ed17f0e0674 140:7bf668fb312f2f4b 54:490e48e9db338af4 256:af2c514af75c0674 96:983677de46483afe 112:8d42f72f2d506039 0:11
h -9 -9 -5 -5 260 10 -9 -9 -144 -144 b30500024beef229 140:24ae1613f1cb40ec 54:31f35dee20b357d4 256:7c6635f8681cf229 96:be6b5ec028542eca 112:1f205451469b296c 0:11
h -2147483648 2147483647 -1073741824 1073741823 137 9 -2147483648 2147483647 0 -16 483b9863dd9c2e08 0:11 0:11 0:11 0:11 0:11 0:11
h 2147483647 -2147483648 1073741823 -1073741824 137 6 2147483647 -2147483648 -16 0 b775f6dfa83a701a 0:11 0:11 0:11 0:11 0:11 0:11
h -100001 77777 -50001 38888 137 10 -100001 77777 -1600016 1244432 fb0d8b154ca7a82b 0:11 0:11 0:11 0:11 0:11 0:11
h0 64/64 64/64
h1 true true 25 true true true true 256 1c76c64b231b9e88
h2 -6 13 -6 13 326 12 -6 13 -96 208 a0d4ce0f38b32f59 0:11 0:11 0:11 0:11 0:11 0:11
h3 true 176 fd6943c3bd8e196c 6a771b7ec1bf0d24
";

    /// `TileDisposalOracle.java`'s output (`./run.sh` writes it to `disposal.txt`).
    const TILE_DISPOSAL_ORACLE: &str = "\
x0 0 0 4 16 true true
x1 4 3 4 16 true true 0[] true
x2 4 4 4 16 true true 1[-1,2,true,false] false
x3 4 5 4 16 true true 2[-1,2,true,false;-1,2,false,false] false
x4 true -1 0 4 16 false false 4 6 4 16 true true 3[-1,2,true,false;-1,2,false,false;-1,2,false,false] false
x5 0 0 4 16 true true -1 4 4 16 false false 4[-1,2,true,false;-1,2,false,false;-1,2,false,false;5,-6,true,false] false
x6 1 4 4 16 true true 0 4 4 16 true true 5[-1,2,true,false;-1,2,false,false;-1,2,false,false;5,-6,true,false;3,3,true,false] false
x7 1 0 0 4 16 true true false false true 0 0 1 1 true true -1 0 1 1 false false false 5[-1,2,true,false;-1,2,false,false;-1,2,false,false;5,-6,true,false;3,3,true,false] 0[]
x8 0 4 4 16 true true -1 0 4 16 false false 6[-1,2,true,false;-1,2,false,false;-1,2,false,false;5,-6,true,false;3,3,true,false;0,0,false,false] false
x9 7[-1,2,true,false;-1,2,false,false;-1,2,false,false;5,-6,true,false;3,3,true,false;0,0,false,false;9,9,true,false] 1[10,10,false,false] false true 0 4 4 16 true true
x10 -1 8 4 16 false false 5 true false -1 8000 4 16 false false 7997
x11 0 0 4 16 true true -1 0 4 16 false false true
";

    /// `TileDisposalOracle`'s `Logged`: logs each `onDispose` (`<regionX>,<regionZ>,<the cache's
    /// entry holds this tile>,<the map's lock is held>`) before the cache's own. The lookup takes
    /// the map's read lock, so it would never return under the write lock: the flag is `false`
    /// whenever the line is logged at all.
    struct Logged {
        cache: Arc<Cache<CacheEntry<'static, Tile>>>,
        events: Mutex<Vec<String>>,
    }

    impl DisposeListener<Tile> for Logged {
        fn on_dispose(&self, tile: &Tile) {
            let entry = self.cache.get(tile.region_id());
            let same = entry.is_some_and(|entry| std::ptr::eq(entry.call(), tile));
            let (rx, rz) = (tile.region_x(), tile.region_z());
            self.events
                .lock()
                .unwrap()
                .push(format!("{rx},{rz},{same},false"));
            self.cache.on_dispose(tile);
        }
    }

    impl Logged {
        /// `new Logged(generator)`: a `NONE` tile cache whose listener is the logger.
        fn new<H, F>(
            generator: &Arc<TileGenerator<H, F>>,
            clock: fn() -> i64,
        ) -> (TileCache<H, F>, Arc<Self>)
        where
            H: HeightmapSource + Send + Sync + 'static,
            F: TileFilters + Send + Sync + 'static,
        {
            let cache = TileCache::with_clock(Arc::clone(generator), ThreadPool::NONE, clock);
            let logged = Arc::new(Self {
                cache: Arc::clone(&cache.cache),
                events: Mutex::new(Vec::new()),
            });
            let listener = Arc::downgrade(&logged);
            generator.set_listener(listener);
            (cache, logged)
        }

        fn events(&self) -> String {
            let events = self.events.lock().unwrap();
            format!("{}[{}]", events.len(), events.join(";"))
        }
    }

    /// `<active> <disposed> <chunkCount> <getChunkCount> <open> <open>`: Engine's two resources
    /// are open until the tile closes (no line reopens a closed tile).
    fn disposal_state(tile: &Tile) -> String {
        let open = tile.active() != -1;
        let (active, disposed) = (tile.active(), tile.disposed());
        let (threshold, count) = (tile.disposal_threshold(), tile.chunk_count());
        format!("{active} {disposed} {threshold} {count} {open} {open}")
    }

    /// The four readers of a size-1 region (2x2 chunks), z rows then x.
    fn region_readers<H, F>(cache: &TileCache<H, F>, rx: i32, rz: i32) -> Vec<CachedChunk>
    where
        H: HeightmapSource + Send + Sync + 'static,
        F: TileFilters + Send + Sync + 'static,
    {
        (0..4)
            .map(|i| {
                cache
                    .get_chunk(rx * 2 + (i & 1), rz * 2 + (i >> 1))
                    .unwrap()
            })
            .collect()
    }

    #[test]
    fn tile_disposal_matches_engine_oracle() {
        static CLOCK: AtomicI64 = AtomicI64::new(0);
        fn clock() -> i64 {
            CLOCK.load(Ordering::SeqCst)
        }
        let terrains = Arc::new(TerrainRegistry::new());
        let kinds = ["none", "coast", "beach", "hills"]
            .map(|name| terrains.get_by_name(name).expect("built-in terrain"));
        let world = || {
            let mut settings = FilterSettings::default();
            settings.erosion.droplets_per_chunk = 20;
            settings.smoothing.iterations = 1;
            let points = WorldControlPoints {
                beach: 0.327,
                ..WorldControlPoints::default()
            };
            let filters =
                WorldFilters::new(12345, settings, Levels::new(256, 63), points, &terrains);
            WorldGenerator::new(FillHeightmap::new(kinds.clone()), filters)
        };
        let generator = |size| {
            Arc::new(
                TileGenerator::builder()
                    .size(size, size)
                    .factory(Arc::clone(&terrains), world())
                    .build(),
            )
        };
        let tiles = generator(1);
        let (dc, log) = Logged::new(&tiles, clock);
        let mut out = String::new();
        let entry = dc.get_tile_at(-1, 2);
        let t = entry.call();
        out += &format!("x0 {}\n", disposal_state(t));
        let r = region_readers(&dc, -1, 2);
        for reader in &r[..3] {
            reader.dispose();
        }
        let has =
            |cache: &TileCache<_, _>, rx, rz| cache.get(Tile::region_id_for(rx, rz)).is_some();
        out += &format!(
            "x1 {} {} {}\n",
            disposal_state(t),
            log.events(),
            has(&dc, -1, 2)
        );
        r[3].dispose();
        out += &format!(
            "x2 {} {} {}\n",
            disposal_state(t),
            log.events(),
            has(&dc, -1, 2)
        );
        r[0].dispose();
        out += &format!(
            "x3 {} {} {}\n",
            disposal_state(t),
            log.events(),
            has(&dc, -1, 2)
        );
        let entry2 = dc.get_tile_at(-1, 2);
        let t2 = entry2.call();
        r[0].dispose();
        out += &format!(
            "x4 {} {} {} {} {}\n",
            !std::ptr::eq(t2, t),
            disposal_state(t2),
            disposal_state(t),
            log.events(),
            has(&dc, -1, 2)
        );
        let entry3 = dc.get_tile_at(5, -6);
        let t3 = entry3.call();
        let r3 = region_readers(&dc, 5, -6);
        r3.iter().for_each(CachedChunk::close);
        let closed = disposal_state(t3);
        for reader in &r3[..3] {
            reader.dispose();
        }
        t3.dispose();
        out += &format!(
            "x5 {closed} {} {} {}\n",
            disposal_state(t3),
            log.events(),
            has(&dc, 5, -6)
        );
        let entry4 = dc.get_tile_at(3, 3);
        let t4 = entry4.call();
        let r4 = region_readers(&dc, 3, 3);
        r4[..3].iter().for_each(CachedChunk::close);
        r4.iter().for_each(CachedChunk::dispose);
        let held = disposal_state(t4);
        r4[3].close();
        out += &format!(
            "x6 {held} {} {} {}\n",
            disposal_state(t4),
            log.events(),
            has(&dc, 3, 3)
        );
        let entry5 = dc.get_tile_at(7, -3);
        CLOCK.store(i64::MAX, Ordering::SeqCst);
        let swept = dc.cache.run();
        CLOCK.store(0, Ordering::SeqCst);
        let sweep = format!(
            "{swept} {} {}",
            disposal_state(entry5.call()),
            has(&dc, 7, -3)
        );
        let small = generator(0);
        let (sc, small_log) = Logged::new(&small, clock);
        let first = sc.get_tile_at(0, 0);
        for x in 1..=256 {
            sc.get_tile_at(x, 0);
        }
        let evicted = format!(
            "{} {} {}",
            has(&sc, 0, 0),
            has(&sc, 1, 0),
            disposal_state(first.call())
        );
        let kept = sc.get_tile_at(1, 0);
        sc.cache.remove(Tile::region_id_for(1, 0));
        out += &format!(
            "x7 {sweep} {evicted} {} {} {} {}\n",
            disposal_state(kept.call()),
            has(&sc, 1, 0),
            log.events(),
            small_log.events()
        );
        let cached00 = dc.get_tile_at(0, 0);
        let zoomed = tiles.generate_zoomed_region(0.0, 0.0, 1.0, false);
        (0..4).for_each(|_| zoomed.dispose());
        out += &format!(
            "x8 {} {} {} {}\n",
            disposal_state(&zoomed),
            disposal_state(cached00.call()),
            log.events(),
            has(&dc, 0, 0)
        );
        let entry6 = dc.get_tile_at(9, 9);
        let (_dd, dd_log) = Logged::new(&tiles, clock);
        let entry7 = dc.get_tile_at(10, 10);
        for _ in 0..4 {
            entry6.call().dispose();
            entry7.call().dispose();
        }
        out += &format!(
            "x9 {} {} {} {} {}\n",
            log.events(),
            dd_log.events(),
            has(&dc, 9, 9),
            has(&dc, 10, 10),
            disposal_state(entry7.call())
        );
        let tiles2 = generator(1);
        let (ec, ec_log) = Logged::new(&tiles2, clock);
        let entry8 = ec.get_tile_at(-4, 6);
        let r8 = region_readers(&ec, -4, 6);
        r8.iter().for_each(CachedChunk::close);
        let barrier = std::sync::Barrier::new(8);
        std::thread::scope(|scope| {
            for i in 0..8 {
                let (reader, barrier) = (&r8[i & 3], &barrier);
                scope.spawn(move || {
                    barrier.wait();
                    reader.dispose();
                });
            }
        });
        let burst = {
            let events = ec_log.events.lock().unwrap();
            // Whether the entry still held the tile depends on the threads' order.
            let region = events.iter().all(|e| e.starts_with("-4,6,"));
            format!("{} {region}", events.len())
        };
        let entry9 = ec.get_tile_at(2, 2);
        ec_log.events.lock().unwrap().clear();
        ec.cache.remove(Tile::region_id_for(2, 2));
        std::thread::scope(|scope| {
            for _ in 0..8 {
                let (tile, barrier) = (entry9.call(), &barrier);
                scope.spawn(move || {
                    barrier.wait();
                    (0..1000).for_each(|_| tile.dispose());
                });
            }
        });
        out += &format!(
            "x10 {} {burst} {} {} {}\n",
            disposal_state(entry8.call()),
            has(&ec, -4, 6),
            disposal_state(entry9.call()),
            ec_log.events.lock().unwrap().len()
        );
        let pooled = Arc::new(
            TileGenerator::builder()
                .size(1, 1)
                .pool(8)
                .factory(Arc::clone(&terrains), world())
                .build(),
        );
        let pc = TileCache::with_clock(Arc::clone(&pooled), pooled.pool.clone(), clock);
        let unread = pc.get_or_compute(Tile::region_id_for(1, 1));
        while !unread.is_done() {
            std::hint::spin_loop();
        }
        pc.cache.remove(Tile::region_id_for(1, 1));
        let late = disposal_state(unread.get());
        let read = pc.get_tile_at(2, 1);
        pc.cache.remove(Tile::region_id_for(2, 1));
        let state = disposal_state(read.call());
        out += &format!("x11 {late} {state} {}\n", !has(&pc, 1, 1));
        assert_eq!(out, TILE_DISPOSAL_ORACLE);
        let entry = ec.get_tile_at(1, 1);
        ec.on_dispose(entry.call());
        assert!(!has(&ec, 1, 1), "onDispose removes the tile's region");
        assert_eq!(entry.call().active(), -1, "and closes its unopened tile");
        let alive = TileCache::with_clock(generator(0), ThreadPool::NONE, clock);
        let entry = alive.get_tile_at(0, 0);
        entry.call().dispose();
        assert_eq!(entry.call().active(), -1, "the cache removed and closed it");
        let dropped = TileCache::with_clock(generator(0), ThreadPool::NONE, clock);
        let entry = dropped.get_tile_at(0, 0);
        drop(dropped);
        entry.call().dispose();
        assert_eq!(
            entry.call().active(),
            0,
            "a dropped cache's tiles tell no one"
        );
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
b 0 0 3 140 ff293731a98fd032 2afe9d3c12774c0d
y 0 0 3 3 20 3a5386a7092fd84 c94e0748cd32a3cd
b 0 0 2 140 ff293731a98fd032 2afe9d3c12774c0d
b 0 0 7 140 ff293731a98fd032 2afe9d3c12774c0d
b 0 0 100 140 ff293731a98fd032 2afe9d3c12774c0d
y 0 0 100 0 143 1d834f8ce5c4fe1 4166d8e307e01385
b 0 1 1 140 ff293731a98fd032 2afe9d3c12774c0d
b 0 1 3 140 ff293731a98fd032 2afe9d3c12774c0d
y 0 1 3 3 20 3a5386a7092fd84 c94e0748cd32a3cd
b 0 1 2 140 ff293731a98fd032 2afe9d3c12774c0d
b 0 1 7 140 ff293731a98fd032 2afe9d3c12774c0d
b 0 1 100 140 ff293731a98fd032 2afe9d3c12774c0d
y 0 1 100 0 143 1d834f8ce5c4fe1 4166d8e307e01385
e 0 0 0 throws ArithmeticException
e 0 0 -2 throws NullPointerException
e 0 1 0 throws ArithmeticException
e 0 1 -2 throws NullPointerException
u 0 - 0 0 TileGenerator 132 933dedba501bf8f1 ff293731a8dcafe2 2afe9d3c12774c0d 2afe9d3c12774c0d
w 0 - 0 0 0 134 8ea15ead27a7ffee 1d834f8cd92c605 4166d8e307e01385
u 0 - 0 1 TileGenerator 132 - ff293731a8dcafe2 2afe9d3c12774c0d 2afe9d3c12774c0d
w 0 - 0 1 1 637 - fefc13f22665e006 29ad7e3985d0e0bc
u 0 - 1 0 TileGenerator 132 933dedba501bf8f1 ff293731a8dcafe2 2afe9d3c12774c0d 2afe9d3c12774c0d
w 0 - 1 0 2 256 6727ea6ac494b891 21f82f7b65700 d25a67c809fc7d0
u 0 - 1 1 TileGenerator 132 - ff293731a8dcafe2 2afe9d3c12774c0d 2afe9d3c12774c0d
w 0 - 1 1 3 8 - 3a5386a6f864af4 c94e0748cd32a3cd
u 0 0 0 0 TileGenerator 132 933dedba501bf8f1 ff293731a8dcafe2 2afe9d3c12774c0d 2afe9d3c12774c0d
w 0 0 0 0 4 388 b8f70426f6f55903 23001b79eacc210 ff2a52d36fd50b5d
u 0 0 0 1 TileGenerator 132 - ff293731a8dcafe2 2afe9d3c12774c0d 2afe9d3c12774c0d
w 0 0 0 1 0 134 - 1d834f8cd92c605 4166d8e307e01385
u 0 0 1 0 TileGenerator 132 933dedba501bf8f1 ff293731a8dcafe2 2afe9d3c12774c0d 2afe9d3c12774c0d
w 0 0 1 0 1 637 4ecaf1596e70d26 fefc13f22665e006 29ad7e3985d0e0bc
u 0 0 1 1 TileGenerator 132 - ff293731a8dcafe2 2afe9d3c12774c0d 2afe9d3c12774c0d
w 0 0 1 1 2 256 - 21f82f7b65700 d25a67c809fc7d0
u 0 -3 0 0 TileGenerator 132 933dedba501bf8f1 ff293731a8dcafe2 2afe9d3c12774c0d 2afe9d3c12774c0d
w 0 -3 0 0 3 8 8f7006778fc0c8d 3a5386a6f864af4 c94e0748cd32a3cd
u 0 -3 0 1 TileGenerator 132 - ff293731a8dcafe2 2afe9d3c12774c0d 2afe9d3c12774c0d
w 0 -3 0 1 4 388 - 23001b79eacc210 ff2a52d36fd50b5d
u 0 -3 1 0 TileGenerator 132 933dedba501bf8f1 ff293731a8dcafe2 2afe9d3c12774c0d 2afe9d3c12774c0d
w 0 -3 1 0 0 134 8ea15ead27a7ffee 1d834f8cd92c605 4166d8e307e01385
u 0 -3 1 1 TileGenerator 132 - ff293731a8dcafe2 2afe9d3c12774c0d 2afe9d3c12774c0d
w 0 -3 1 1 1 637 - fefc13f22665e006 29ad7e3985d0e0bc
u 0 1 0 0 TileGeneratorBatched 140 e61f5dd69ccf3871 ff293731a98fd032 2afe9d3c12774c0d 2afe9d3c12774c0d
w 0 1 0 0 2 264 1f60d3571d48855 21f82f8697838 d25a67c809fc7d0
u 0 1 0 1 TileGeneratorBatched 140 - ff293731a98fd032 2afe9d3c12774c0d 2afe9d3c12774c0d
w 0 1 0 1 3 20 - 3a5386a7092fd84 c94e0748cd32a3cd
u 0 1 1 0 TileGeneratorStriped 140 e61f5dd69ccf3871 ff293731a98fd032 2afe9d3c12774c0d 2afe9d3c12774c0d
w 0 1 1 0 4 391 17e0ef8926dd9870 23001b8816f54d4 ff2a52d36fd50b5d
u 0 1 1 1 TileGeneratorStriped 140 - ff293731a98fd032 2afe9d3c12774c0d 2afe9d3c12774c0d
w 0 1 1 1 0 143 - 1d834f8ce5c4fe1 4166d8e307e01385
u 0 4 0 0 TileGeneratorBatched 140 e61f5dd69ccf3871 ff293731a98fd032 2afe9d3c12774c0d 2afe9d3c12774c0d
w 0 4 0 0 1 640 379305f14f2fd451 fefc13f226a90bf4 29ad7e3985d0e0bc
u 0 4 0 1 TileGeneratorBatched 140 - ff293731a98fd032 2afe9d3c12774c0d 2afe9d3c12774c0d
w 0 4 0 1 2 264 - 21f82f8697838 d25a67c809fc7d0
u 0 4 1 0 TileGeneratorStriped 140 e61f5dd69ccf3871 ff293731a98fd032 2afe9d3c12774c0d 2afe9d3c12774c0d
w 0 4 1 0 3 20 7699b0f6fa05ff4d 3a5386a7092fd84 c94e0748cd32a3cd
u 0 4 1 1 TileGeneratorStriped 140 - ff293731a98fd032 2afe9d3c12774c0d 2afe9d3c12774c0d
w 0 4 1 1 4 391 - 23001b8816f54d4 ff2a52d36fd50b5d
l 0 false 0 11 132 933dedba501bf8f1 -1 2 2afe9d3c12774c0d true 0 11 true
m 0 false 0 11 134 8ea15ead27a7ffee 0 0 4166d8e307e01385 true 0 11 true
g 1 397 7245952d37687c55 e76a63e92f8379dd
z 1 0 0 0 387 d49fcfd0bb2413f2 835d8bb2fa3b2820
z 1 1 0 0 1339 11b697d16848b786 567dacf67f971c65
z 1 2 0 0 400 cd67b6f5d8dbd049 245b91724bc38581
z 1 3 0 0 12 375f578adf39af4b c24985dd40cbb39
z 1 4 0 0 780 67386badb7371713 ff26c135e2959741
b 1 0 1 412 430d669402ba99e e76a63e92f8379dd
b 1 0 3 412 430d669402ba99e e76a63e92f8379dd
y 1 0 3 4 788 4ec03c40ff6e3de ff26c135e2959741
b 1 0 2 412 430d669402ba99e e76a63e92f8379dd
b 1 0 7 412 430d669402ba99e e76a63e92f8379dd
b 1 0 100 412 430d669402ba99e e76a63e92f8379dd
y 1 0 100 1 1344 fe886cc8d526ea84 567dacf67f971c65
b 1 1 1 412 430d669402ba99e e76a63e92f8379dd
b 1 1 3 412 430d669402ba99e e76a63e92f8379dd
y 1 1 3 4 788 4ec03c40ff6e3de ff26c135e2959741
b 1 1 2 412 430d669402ba99e e76a63e92f8379dd
b 1 1 7 412 430d669402ba99e e76a63e92f8379dd
b 1 1 100 412 430d669402ba99e e76a63e92f8379dd
y 1 1 100 1 1344 fe886cc8d526ea84 567dacf67f971c65
l 1 false 0 11 397 7245952d37687c55 3 -5 e76a63e92f8379dd true 0 11 true
m 1 false 0 11 1339 11b697d16848b786 0 0 567dacf67f971c65 true 0 11 true
g 2 64 1dec756bf463a431 40bc3ec1ab771b58
z 2 0 0 0 64 83a19de368360031 bd0ab1842c50e7af
z 2 1 0 0 128 3822735c75797611 cb3b77fd7a9dc52
z 2 2 0 0 4 7984c67e49e48d73 4756c143b5e81e07
z 2 3 0 0 4 ae3ae2c61568fccf a6e0dce0067b12e9
z 2 4 0 0 66 d5ffa2bb28613a2a 51601f1cf26142c
b 2 0 1 66 76f754be1ac50e 40bc3ec1ab771b58
b 2 0 3 66 76f754be1ac50e 40bc3ec1ab771b58
y 2 0 3 0 66 760d40901c86c2 bd0ab1842c50e7af
b 2 0 2 66 76f754be1ac50e 40bc3ec1ab771b58
b 2 0 7 66 76f754be1ac50e 40bc3ec1ab771b58
b 2 0 100 66 76f754be1ac50e 40bc3ec1ab771b58
y 2 0 100 2 4 fffdc4acb8ae1c9c 4756c143b5e81e07
b 2 1 1 66 76f754be1ac50e 40bc3ec1ab771b58
b 2 1 3 66 76f754be1ac50e 40bc3ec1ab771b58
y 2 1 3 0 66 760d40901c86c2 bd0ab1842c50e7af
b 2 1 2 66 76f754be1ac50e 40bc3ec1ab771b58
b 2 1 7 66 76f754be1ac50e 40bc3ec1ab771b58
b 2 1 100 66 76f754be1ac50e 40bc3ec1ab771b58
y 2 1 100 2 4 fffdc4acb8ae1c9c 4756c143b5e81e07
u 2 - 0 0 TileGenerator 64 1dec756bf463a431 76f754bdedfca0 40bc3ec1ab771b58 40bc3ec1ab771b58
w 2 - 0 0 2 4 7984c67e49e48d73 fffdc4acb8ae1c9c 4756c143b5e81e07
u 2 - 0 1 TileGenerator 64 - 76f754bdedfca0 40bc3ec1ab771b58 40bc3ec1ab771b58
w 2 - 0 1 3 4 - e9bf5b962e5b7a a6e0dce0067b12e9
u 2 - 1 0 TileGenerator 64 1dec756bf463a431 76f754bdedfca0 40bc3ec1ab771b58 40bc3ec1ab771b58
w 2 - 1 0 4 66 d5ffa2bb28613a2a 8c0064c0977191 51601f1cf26142c
u 2 - 1 1 TileGenerator 64 - 76f754bdedfca0 40bc3ec1ab771b58 40bc3ec1ab771b58
w 2 - 1 1 0 64 - 760d408fefbd60 bd0ab1842c50e7af
u 2 0 0 0 TileGenerator 64 1dec756bf463a431 76f754bdedfca0 40bc3ec1ab771b58 40bc3ec1ab771b58
w 2 0 0 0 1 128 3822735c75797611 ff90805455b5c7a0 cb3b77fd7a9dc52
u 2 0 0 1 TileGenerator 64 - 76f754bdedfca0 40bc3ec1ab771b58 40bc3ec1ab771b58
w 2 0 0 1 2 4 - fffdc4acb8ae1c9c 4756c143b5e81e07
u 2 0 1 0 TileGenerator 64 1dec756bf463a431 76f754bdedfca0 40bc3ec1ab771b58 40bc3ec1ab771b58
w 2 0 1 0 3 4 ae3ae2c61568fccf e9bf5b962e5b7a a6e0dce0067b12e9
u 2 0 1 1 TileGenerator 64 - 76f754bdedfca0 40bc3ec1ab771b58 40bc3ec1ab771b58
w 2 0 1 1 4 66 - 8c0064c0977191 51601f1cf26142c
u 2 -3 0 0 TileGenerator 64 1dec756bf463a431 76f754bdedfca0 40bc3ec1ab771b58 40bc3ec1ab771b58
w 2 -3 0 0 0 64 83a19de368360031 760d408fefbd60 bd0ab1842c50e7af
u 2 -3 0 1 TileGenerator 64 - 76f754bdedfca0 40bc3ec1ab771b58 40bc3ec1ab771b58
w 2 -3 0 1 1 128 - ff90805455b5c7a0 cb3b77fd7a9dc52
u 2 -3 1 0 TileGenerator 64 1dec756bf463a431 76f754bdedfca0 40bc3ec1ab771b58 40bc3ec1ab771b58
w 2 -3 1 0 2 4 7984c67e49e48d73 fffdc4acb8ae1c9c 4756c143b5e81e07
u 2 -3 1 1 TileGenerator 64 - 76f754bdedfca0 40bc3ec1ab771b58 40bc3ec1ab771b58
w 2 -3 1 1 3 4 - e9bf5b962e5b7a a6e0dce0067b12e9
u 2 1 0 0 TileGeneratorBatched 66 31382c358b7f3b11 76f754be1ac50e 40bc3ec1ab771b58 40bc3ec1ab771b58
w 2 1 0 0 4 67 deedfbcef3f19ba 8c00650c2da25e 51601f1cf26142c
u 2 1 0 1 TileGeneratorBatched 66 - 76f754be1ac50e 40bc3ec1ab771b58 40bc3ec1ab771b58
w 2 1 0 1 0 66 - 760d40901c86c2 bd0ab1842c50e7af
u 2 1 1 0 TileGeneratorStriped 66 31382c358b7f3b11 76f754be1ac50e 40bc3ec1ab771b58 40bc3ec1ab771b58
w 2 1 1 0 1 128 3822735c75797611 ff90805455b5c7a0 cb3b77fd7a9dc52
u 2 1 1 1 TileGeneratorStriped 66 - 76f754be1ac50e 40bc3ec1ab771b58 40bc3ec1ab771b58
w 2 1 1 1 2 4 - fffdc4acb8ae1c9c 4756c143b5e81e07
u 2 4 0 0 TileGeneratorBatched 66 31382c358b7f3b11 76f754be1ac50e 40bc3ec1ab771b58 40bc3ec1ab771b58
w 2 4 0 0 3 6 d15fcfd95dd2cb4f e9bf5b965b23e8 a6e0dce0067b12e9
u 2 4 0 1 TileGeneratorBatched 66 - 76f754be1ac50e 40bc3ec1ab771b58 40bc3ec1ab771b58
w 2 4 0 1 4 67 - 8c00650c2da25e 51601f1cf26142c
u 2 4 1 0 TileGeneratorStriped 66 31382c358b7f3b11 76f754be1ac50e 40bc3ec1ab771b58 40bc3ec1ab771b58
w 2 4 1 0 0 66 de31c0b5ca85951 760d40901c86c2 bd0ab1842c50e7af
u 2 4 1 1 TileGeneratorStriped 66 - 76f754be1ac50e 40bc3ec1ab771b58 40bc3ec1ab771b58
w 2 4 1 1 1 128 - ff90805455b5c7a0 cb3b77fd7a9dc52
l 2 false 0 11 64 1dec756bf463a431 0 0 40bc3ec1ab771b58 true 0 11 true
m 2 false 0 11 4 7984c67e49e48d73 0 0 4756c143b5e81e07 true 0 11 true
g 3 326 e3c4cdf7c5eefaee 6f363908c2b37e18
z 3 0 0 0 18 3b45073f941ef450 6ca6c7d6745fb36a
z 3 1 0 0 876 c2eea373f1f90a33 339683019cd47a84
z 3 2 0 0 176 c1be527b1c6e9de9 ee52c23ae4267780
z 3 3 0 0 10 e5baee091137038c c7d462f17993a8db
z 3 4 0 0 490 10079d51f1cdcca3 b4a4cebe6cb0976c
b 3 0 1 338 fd5fd1e315136131 6f363908c2b37e18
b 3 0 3 338 fd5fd1e315136131 6f363908c2b37e18
y 3 0 3 1 880 fec0d50e0c2b5520 339683019cd47a84
b 3 0 2 338 fd5fd1e315136131 6f363908c2b37e18
b 3 0 7 338 fd5fd1e315136131 6f363908c2b37e18
b 3 0 100 338 fd5fd1e315136131 6f363908c2b37e18
y 3 0 100 3 30 5b1a07cd4c1ae73 c7d462f17993a8db
b 3 1 1 338 fd5fd1e315136131 6f363908c2b37e18
b 3 1 3 338 fd5fd1e315136131 6f363908c2b37e18
y 3 1 3 1 880 fec0d50e0c2b5520 339683019cd47a84
b 3 1 2 338 fd5fd1e315136131 6f363908c2b37e18
b 3 1 7 338 fd5fd1e315136131 6f363908c2b37e18
b 3 1 100 338 fd5fd1e315136131 6f363908c2b37e18
y 3 1 100 3 30 5b1a07cd4c1ae73 c7d462f17993a8db
u 3 - 0 0 TileGenerator 326 e3c4cdf7c5eefaee fd5fd1e31406b5b7 6f363908c2b37e18 6f363908c2b37e18
w 3 - 0 0 3 10 e5baee091137038c 5b1a07cd301da31 c7d462f17993a8db
u 3 - 0 1 TileGenerator 326 - fd5fd1e31406b5b7 6f363908c2b37e18 6f363908c2b37e18
w 3 - 0 1 4 490 - 36b028c8db94a8c b4a4cebe6cb0976c
u 3 - 1 0 TileGenerator 326 e3c4cdf7c5eefaee fd5fd1e31406b5b7 6f363908c2b37e18 6f363908c2b37e18
w 3 - 1 0 0 18 3b45073f941ef450 2e1d26995cfc911 6ca6c7d6745fb36a
u 3 - 1 1 TileGenerator 326 - fd5fd1e31406b5b7 6f363908c2b37e18 6f363908c2b37e18
w 3 - 1 1 1 876 - fec0d50e0bd1c540 339683019cd47a84
u 3 0 0 0 TileGenerator 326 e3c4cdf7c5eefaee fd5fd1e31406b5b7 6f363908c2b37e18 6f363908c2b37e18
w 3 0 0 0 2 176 c1be527b1c6e9de9 781538f7545d0 ee52c23ae4267780
u 3 0 0 1 TileGenerator 326 - fd5fd1e31406b5b7 6f363908c2b37e18 6f363908c2b37e18
w 3 0 0 1 3 10 - 5b1a07cd301da31 c7d462f17993a8db
u 3 0 1 0 TileGenerator 326 e3c4cdf7c5eefaee fd5fd1e31406b5b7 6f363908c2b37e18 6f363908c2b37e18
w 3 0 1 0 4 490 10079d51f1cdcca3 36b028c8db94a8c b4a4cebe6cb0976c
u 3 0 1 1 TileGenerator 326 - fd5fd1e31406b5b7 6f363908c2b37e18 6f363908c2b37e18
w 3 0 1 1 0 18 - 2e1d26995cfc911 6ca6c7d6745fb36a
u 3 -3 0 0 TileGenerator 326 e3c4cdf7c5eefaee fd5fd1e31406b5b7 6f363908c2b37e18 6f363908c2b37e18
w 3 -3 0 0 1 876 c2eea373f1f90a33 fec0d50e0bd1c540 339683019cd47a84
u 3 -3 0 1 TileGenerator 326 - fd5fd1e31406b5b7 6f363908c2b37e18 6f363908c2b37e18
w 3 -3 0 1 2 176 - 781538f7545d0 ee52c23ae4267780
u 3 -3 1 0 TileGenerator 326 e3c4cdf7c5eefaee fd5fd1e31406b5b7 6f363908c2b37e18 6f363908c2b37e18
w 3 -3 1 0 3 10 e5baee091137038c 5b1a07cd301da31 c7d462f17993a8db
u 3 -3 1 1 TileGenerator 326 - fd5fd1e31406b5b7 6f363908c2b37e18 6f363908c2b37e18
w 3 -3 1 1 4 490 - 36b028c8db94a8c b4a4cebe6cb0976c
u 3 1 0 0 TileGeneratorBatched 338 87bde3f5fa751ace fd5fd1e315136131 6f363908c2b37e18 6f363908c2b37e18
w 3 1 0 0 0 30 78250de8f9e4dbf5 2e1d26996dc80e4 6ca6c7d6745fb36a
u 3 1 0 1 TileGeneratorBatched 338 - fd5fd1e315136131 6f363908c2b37e18 6f363908c2b37e18
w 3 1 0 1 1 880 - fec0d50e0c2b5520 339683019cd47a84
u 3 1 1 0 TileGeneratorStriped 338 87bde3f5fa751ace fd5fd1e315136131 6f363908c2b37e18 6f363908c2b37e18
w 3 1 1 0 2 184 de53149a01189ea5 7815390286708 ee52c23ae4267780
u 3 1 1 1 TileGeneratorStriped 338 - fd5fd1e315136131 6f363908c2b37e18 6f363908c2b37e18
w 3 1 1 1 3 30 - 5b1a07cd4c1ae73 c7d462f17993a8db
u 3 4 0 0 TileGeneratorBatched 338 30c880544cc985b0 fd5fd1e315136131 6f363908c2b37e18 6f363908c2b37e18
w 3 4 0 0 4 496 b12c823d4a6b6315 36b028e533e6f86 b4a4cebe6cb0976c
u 3 4 0 1 TileGeneratorBatched 338 - fd5fd1e315136131 6f363908c2b37e18 6f363908c2b37e18
w 3 4 0 1 0 30 - 2e1d26996dc80e4 6ca6c7d6745fb36a
u 3 4 1 0 TileGeneratorStriped 338 87bde3f5fa751ace fd5fd1e315136131 6f363908c2b37e18 6f363908c2b37e18
w 3 4 1 0 1 880 37671de9185e4711 fec0d50e0c2b5520 339683019cd47a84
u 3 4 1 1 TileGeneratorStriped 338 - fd5fd1e315136131 6f363908c2b37e18 6f363908c2b37e18
w 3 4 1 1 2 184 - 7815390286708 ee52c23ae4267780
l 3 false 0 11 326 e3c4cdf7c5eefaee -7 9 6f363908c2b37e18 true 0 11 true
m 3 false 0 11 10 e5baee091137038c 0 0 c7d462f17993a8db true 0 11 true
g 4 1568 b599986399fb3e01 9a47ad4f7c38f0ee
z 4 0 0 0 1538 5fa0efaac8bcc9e1 c7ce55c3d1993304
z 4 1 0 0 5354 49faaa1e08c16c6d 57f2964297284da4
z 4 2 0 0 1568 cdf03463f768fa01 46ee6896e1099572
z 4 3 0 0 70 ec220899b350f0e4 28f7541d10beeeef
z 4 4 0 0 2733 fb4116edd58af70f 970936609b0cd3ac
b 4 0 1 1648 10e3e60eaa993958 9a47ad4f7c38f0ee
b 4 0 3 1648 10e3e60eaa993958 9a47ad4f7c38f0ee
y 4 0 3 2 1648 74713a7c086b10 46ee6896e1099572
b 4 0 2 1648 10e3e60eaa993958 9a47ad4f7c38f0ee
b 4 0 7 1648 10e3e60eaa993958 9a47ad4f7c38f0ee
b 4 0 100 1648 10e3e60eaa993958 9a47ad4f7c38f0ee
y 4 0 100 4 2783 13b00ea32c87c9a8 970936609b0cd3ac
b 4 1 1 1648 10e3e60eaa993958 9a47ad4f7c38f0ee
b 4 1 3 1648 10e3e60eaa993958 9a47ad4f7c38f0ee
y 4 1 3 2 1648 74713a7c086b10 46ee6896e1099572
b 4 1 2 1648 10e3e60eaa993958 9a47ad4f7c38f0ee
b 4 1 7 1648 10e3e60eaa993958 9a47ad4f7c38f0ee
b 4 1 100 1648 10e3e60eaa993958 9a47ad4f7c38f0ee
y 4 1 100 4 2783 13b00ea32c87c9a8 970936609b0cd3ac
l 4 false 0 11 1568 b599986399fb3e01 1 -1 9a47ad4f7c38f0ee true 0 11 true
m 4 false 0 11 2733 fb4116edd58af70f 0 0 970936609b0cd3ac true 0 11 true
g 5 137 d84abfe0adb754b 4c8e54bd50b0358b
z 5 0 0 0 134 8ea15ead27a7ffee f7d66956f95d7b09
z 5 1 0 0 637 4ecaf1596e70d26 19eab031c83bcbf7
z 5 2 0 0 256 6727ea6ac494b891 26501176026dc54e
z 5 3 0 0 8 8f7006778fc0c8d c0702424e84aeb1e
z 5 4 0 0 388 b8f70426f6f55903 2b01bed53f599881
b 5 0 1 143 e47d7e7dd561fa 4c8e54bd50b0358b
b 5 0 3 143 e47d7e7dd561fa 4c8e54bd50b0358b
y 5 0 3 3 20 3a5386a7092fd84 c0702424e84aeb1e
b 5 0 2 143 e47d7e7dd561fa 4c8e54bd50b0358b
b 5 0 7 143 e47d7e7dd561fa 4c8e54bd50b0358b
b 5 0 100 143 e47d7e7dd561fa 4c8e54bd50b0358b
y 5 0 100 0 143 1d834f8ce5c4fe1 f7d66956f95d7b09
b 5 1 1 143 e47d7e7dd561fa 4c8e54bd50b0358b
b 5 1 3 143 e47d7e7dd561fa 4c8e54bd50b0358b
y 5 1 3 3 20 3a5386a7092fd84 c0702424e84aeb1e
b 5 1 2 143 e47d7e7dd561fa 4c8e54bd50b0358b
b 5 1 7 143 e47d7e7dd561fa 4c8e54bd50b0358b
b 5 1 100 143 e47d7e7dd561fa 4c8e54bd50b0358b
y 5 1 100 0 143 1d834f8ce5c4fe1 f7d66956f95d7b09
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
d 0 TileGenerator 1 16 16 -1 2 2 9ceef3334738d850 184eae53c5bf6135
d 1 TileGenerator 2 32 32 -1 2 64 84688545135fa31 d9741ee0027beed3
d 2 TileGenerator 1 16 16 -1 2 2 9ceef3334738d850 184eae53c5bf6135
n throws NullPointerException
";

    /// `TileAsyncOracle.java`'s `Gated`: [`FillHeightmap`] that can hold the next fill at its first
    /// `applyBase` until released (to watch a pending tile), records the thread running it, and
    /// panics (Engine: throws) while `fail` is set, counting those runs.
    struct Gated {
        inner: FillHeightmap,
        thread: Mutex<Option<(std::thread::ThreadId, Option<String>)>>,
        gate: Mutex<Option<(mpsc::Sender<()>, mpsc::Receiver<()>)>>,
        fail: AtomicBool,
        runs: AtomicI32,
    }

    impl Gated {
        fn new(kinds: [Terrain; 4]) -> Self {
            Self {
                inner: FillHeightmap::new(kinds),
                thread: Mutex::new(None),
                gate: Mutex::new(None),
                fail: AtomicBool::new(false),
                runs: AtomicI32::new(0),
            }
        }

        /// Holds the next fill's first `apply_base`: returns the channel it signals once there and
        /// the one that releases it.
        fn gate(&self) -> (mpsc::Receiver<()>, mpsc::Sender<()>) {
            let (started, at_gate) = mpsc::channel();
            let (release, released) = mpsc::channel();
            *self.gate.lock().unwrap() = Some((started, released));
            (at_gate, release)
        }

        /// `caller` when the last fill ran on this thread, `task` on a `TF-Task-Worker-<n>`.
        fn place(&self) -> String {
            let (id, name) = self.thread.lock().unwrap().clone().expect("a fill ran");
            if id == std::thread::current().id() {
                return "caller".into();
            }
            let name = name.unwrap_or_default();
            if name.starts_with("TF-Task-Worker-") {
                "task".into()
            } else {
                name
            }
        }
    }

    impl HeightmapSource for Gated {
        fn apply_base(&self, cell: &mut Cell, x: f32, z: f32) {
            if self.fail.load(Ordering::SeqCst) {
                self.runs.fetch_add(1, Ordering::SeqCst);
                panic!("gated failure");
            }
            let gate = self.gate.lock().unwrap().take();
            let current = std::thread::current();
            *self.thread.lock().unwrap() = Some((current.id(), current.name().map(str::to_owned)));
            if let Some((started, release)) = gate {
                started.send(()).unwrap();
                release.recv().unwrap();
            }
            self.inner.apply_base(cell, x, z);
        }

        fn rivermap(&self, x: i32, z: i32) -> Arc<Rivermap> {
            self.inner.rivermap(x, z)
        }

        fn apply_rivers(&self, cell: &mut Cell, x: f32, z: f32, map: &Rivermap) {
            self.inner.apply_rivers(cell, x, z, map);
        }

        fn apply_climate(&self, cell: &mut Cell, x: f32, z: f32) {
            self.inner.apply_climate(cell, x, z);
        }
    }

    /// `TileAsyncOracle`'s `Log`: the regions of the tiles disposed to it.
    #[derive(Default)]
    struct Log(Mutex<Vec<String>>);

    impl DisposeListener<Tile> for Log {
        fn on_dispose(&self, tile: &Tile) {
            let line = format!("{},{}", tile.region_x(), tile.region_z());
            self.0.lock().unwrap().push(line);
        }
    }

    impl Log {
        fn events(&self) -> String {
            let events = self.0.lock().unwrap();
            format!("{}[{}]", events.len(), events.join(";"))
        }
    }

    /// Replays `TileAsyncOracle.java` (`.local/terraforged/oracles/tile_cache/`, `./run.sh` writes
    /// `async.txt`): Engine 0.3.0's `TileGeneratorAsync` from `TileGenerator.async()` over the stub
    /// heightmap (held at a gate, failing on demand) and real filters. `a0`/`a1`: on `NONE`
    /// `async()` keeps the generator's pool and returns itself, `chunkToRegion` is the generator's,
    /// a tile is done at once, runs nothing until `get`, then runs on the caller's thread and is
    /// bit for bit the synchronous `generateRegion`'s (regular and zoomed); `a2`/`a3`: on
    /// `create(8)` `getTile` returns while a task thread is still filling the tile (not done), and
    /// `get` waits for it; `a4`: on the pool a failed task fails every `get` without rerunning, on
    /// `NONE` every `get` reruns it; `a5`: `setListener` delegates to the generator, and a tile
    /// takes the listener the generator has when its task runs; `a6`: `cached()` is a cache over the
    /// wrapped generator on the factory's pool that becomes the generator's listener, so an async
    /// tile's disposal removes the region's cached entry; `a7`: batched and striped generators'
    /// async tiles equal their synchronous ones; `a8`: eight calls for one region make eight tiles.
    #[test]
    fn async_tile_factory_matches_engine_oracle() {
        let terrains = Arc::new(TerrainRegistry::new());
        let kinds = ["none", "coast", "beach", "hills"]
            .map(|name| terrains.get_by_name(name).expect("built-in terrain"));
        let world = || {
            let mut settings = FilterSettings::default();
            settings.erosion.droplets_per_chunk = 20;
            settings.smoothing.iterations = 1;
            let points = WorldControlPoints {
                beach: 0.327,
                ..WorldControlPoints::default()
            };
            let filters =
                WorldFilters::new(12345, settings, Levels::new(256, 63), points, &terrains);
            WorldGenerator::new(Gated::new(kinds.clone()), filters)
        };
        let builder = || {
            TileGenerator::builder()
                .size(1, 1)
                .factory(Arc::clone(&terrains), world())
        };
        let measured = |heightmap: &Gated, tile: &Tile| {
            let (lookups, trace) = heightmap.inner.take();
            let (rx, rz) = (tile.region_x(), tile.region_z());
            format!("{lookups} {trace:x} {rx} {rz} {:x}", fold_tile(tile))
        };
        // Engine's `NullPointerException` from `FutureAdapter.call` is a panic here.
        let fails = |entry: &CacheEntry<'static, Tile>| {
            let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
                entry.get();
            }));
            if result.is_ok() {
                "ok"
            } else {
                "NullPointerException"
            }
        };
        let dispose_all = |tile: &Tile| (0..4).for_each(|_| tile.dispose());
        let pool8 = ThreadPool::create(8);
        let none = Arc::new(builder().build());
        let pooled = Arc::new(builder().pool(8).build());
        let an = Arc::clone(&none).asynchronous();
        let ap = Arc::clone(&pooled).asynchronous();
        let (hn, hp) = (none.generator().heightmap(), pooled.generator().heightmap());
        let mut out = String::new();

        let e = an.get_tile(-1, 2);
        let before = format!(
            "{} {}",
            e.is_done(),
            hn.inner.lookups.load(Ordering::SeqCst)
        );
        let t = e.get();
        let line = measured(hn, t);
        let sync = measured(hn, &none.generate_region(-1, 2));
        out += &format!(
            "a0 true {} {} {} {} {before} {} {line} {sync} {} {}\n",
            std::ptr::eq(an.asynchronous(), &an),
            an.thread_pool.size() == ThreadPool::NONE.size(),
            an.chunk_to_region(-3),
            an.chunk_to_region(i32::MIN),
            hn.place(),
            std::ptr::eq(e.get(), t),
            hn.inner.lookups.load(Ordering::SeqCst)
        );
        let e = an.get_zoomed_tile(100.0, -50.0, 0.5, false);
        let before = format!(
            "{} {}",
            e.is_done(),
            hn.inner.lookups.load(Ordering::SeqCst)
        );
        let line = measured(hn, e.get());
        let sync = measured(hn, &none.generate_zoomed_region(100.0, -50.0, 0.5, false));
        out += &format!("a1 {before} {} {line} {sync}\n", hn.place());

        let (at_gate, release) = hp.gate();
        let e = ap.get_tile(5, -6);
        at_gate.recv().unwrap();
        let pending = format!("{} {}", e.is_done(), hp.place());
        release.send(()).unwrap();
        let t = e.get();
        let line = measured(hp, t);
        let sync = measured(hp, &pooled.generate_region(5, -6));
        out += &format!(
            "a2 {} {pending} {} {line} {sync} {}\n",
            ap.thread_pool.size() == pool8.size(),
            e.is_done(),
            std::ptr::eq(e.get(), t)
        );
        let (at_gate, release) = hp.gate();
        let e = ap.get_zoomed_tile(-37.25, 12.5, 2.0, true);
        at_gate.recv().unwrap();
        let pending = format!("{} {}", e.is_done(), hp.place());
        release.send(()).unwrap();
        let line = measured(hp, e.get());
        let sync = measured(hp, &pooled.generate_zoomed_region(-37.25, 12.5, 2.0, true));
        out += &format!("a3 {pending} {} {line} {sync}\n", e.is_done());

        hp.fail.store(true, Ordering::SeqCst);
        let e = ap.get_tile(3, 3);
        let first = fails(&e);
        let runs = |h: &Gated| h.runs.load(Ordering::SeqCst);
        let mut pf = format!("{first} {} {} {}", e.is_done(), fails(&e), runs(hp));
        hp.fail.store(false, Ordering::SeqCst);
        pf += &format!(" {} {}", fails(&e), runs(hp));
        hn.fail.store(true, Ordering::SeqCst);
        let e = an.get_tile(3, 3);
        let mut nf = format!("{} {}", e.is_done(), runs(hn));
        nf += &format!(" {} {}", fails(&e), runs(hn));
        nf += &format!(" {} {}", fails(&e), runs(hn));
        hn.fail.store(false, Ordering::SeqCst);
        hn.inner.take();
        nf += &format!(" {} {}", fails(&e), runs(hn));
        nf += &format!(" {}", measured(hn, e.get()));
        nf += &format!(" {}", measured(hn, &none.generate_region(3, 3)));
        out += &format!("a4 {pf} {nf}\n");

        let (l1, l2, l3) = (
            Arc::new(Log::default()),
            Arc::new(Log::default()),
            Arc::new(Log::default()),
        );
        let weak = |log: &Arc<Log>| -> TileListener { Arc::<Log>::downgrade(log) };
        an.set_listener(weak(&l1));
        let delegated = none
            .listener
            .read()
            .unwrap()
            .as_ref()
            .is_some_and(|listener| std::ptr::addr_eq(listener.as_ptr(), Arc::as_ptr(&l1)));
        let e = an.get_tile(1, -1);
        an.set_listener(weak(&l2));
        dispose_all(e.get());
        dispose_all(an.get_tile(2, -2).get());
        dispose_all(&none.generate_region(4, 4));
        ap.set_listener(weak(&l3));
        dispose_all(ap.get_tile(-3, 6).get());
        hn.inner.take();
        hp.inner.take();
        out += &format!(
            "a5 {delegated} {} {} {}\n",
            l1.events(),
            l2.events(),
            l3.events()
        );

        let (cn, cp) = (an.cached(), ap.cached());
        let other = AsyncTileGenerator::new(Arc::clone(&none), pool8.clone());
        let co = other.cached();
        let shape = |cache: &TileCache<Gated, WorldFilters>, generator, pool: &ThreadPool| {
            let same = Arc::ptr_eq(&cache.generator, generator);
            let size = cache.thread_pool.size() == pool.size();
            format!("{same} {size} {}", cache.can_queue)
        };
        let shape = [
            shape(&cn, &none, &ThreadPool::NONE),
            shape(&cp, &pooled, &pool8),
            shape(&co, &none, &pool8),
        ]
        .join(" ");
        let cached_tile = cp.get_tile_at(4, -5);
        let async_tile = ap.get_tile(4, -5);
        let distinct = !std::ptr::eq(cached_tile.call(), async_tile.get());
        let equal = fold_tile(cached_tile.call()) == fold_tile(async_tile.call());
        let id = Tile::region_id_for(4, -5);
        let had = cp.get(id).is_some();
        dispose_all(async_tile.call());
        let removed = cp.get(id).is_none();
        other.get_tile(0, 1).get();
        let place = hn.place();
        hn.inner.take();
        hp.inner.take();
        out += &format!(
            "a6 {shape} {distinct} {equal} {had} {removed} {place} {}\n",
            l3.events()
        );

        out += "a7";
        for striped in [false, true] {
            let mut builder = builder().batch(3).pool(8);
            if striped {
                builder = builder.striped();
            }
            let g = Arc::new(builder.build());
            let a = Arc::clone(&g).asynchronous();
            let at = fold_tile(a.get_tile(-7, 9).get());
            let st = fold_tile(&g.generate_region(-7, 9));
            let az = fold_tile(a.get_zoomed_tile(1e9, -1e9, -3.0, false).get());
            let sz = fold_tile(&g.generate_zoomed_region(1e9, -1e9, -3.0, false));
            let class = match g.kind {
                Kind::Plain => "TileGenerator",
                Kind::Batched => "TileGeneratorBatched",
                Kind::Striped => "TileGeneratorStriped",
            };
            out += &format!(" {class} {at:x} {} {az:x} {}", at == st, az == sz);
        }
        out += "\n";

        hp.inner.take();
        let entries: Vec<_> = (0..8).map(|_| ap.get_tile(-1, -1)).collect();
        let tiles: Vec<&Tile> = entries.iter().map(|entry| entry.get()).collect();
        let distinct = (0..8)
            .filter(|&i| (0..i).all(|j| !std::ptr::eq(tiles[i], tiles[j])))
            .count();
        let fold = fold_tile(tiles[0]);
        let same = tiles.iter().all(|tile| fold_tile(tile) == fold);
        let (lookups, _) = hp.inner.take();
        out += &format!("a8 {distinct} {same} {lookups} {fold:x}\n");
        assert_eq!(out, TILE_ASYNC_ORACLE);
    }

    /// `TileAsyncOracle.java`'s output (`./run.sh` writes it to `async.txt`).
    const TILE_ASYNC_ORACLE: &str = "\
a0 true true true -2 -1073741824 true 0 caller 132 933dedba501bf8f1 -1 2 2afe9d3c12774c0d 132 933dedba501bf8f1 -1 2 2afe9d3c12774c0d true 0
a1 true 0 caller 134 8ea15ead27a7ffee 0 0 4166d8e307e01385 134 8ea15ead27a7ffee 0 0 4166d8e307e01385
a2 true false task true 137 7e3196318cb259b8 5 -6 bdd7e0198d0c33c3 137 7e3196318cb259b8 5 -6 bdd7e0198d0c33c3 true
a3 false task true 637 4ecaf1596e70d26 0 0 29ad7e3985d0e0bc 637 4ecaf1596e70d26 0 0 29ad7e3985d0e0bc
a4 NullPointerException true NullPointerException 1 NullPointerException 1 true 0 NullPointerException 1 NullPointerException 2 ok 2 137 8c625cfff37cd90c 3 3 824d1c7dad7a86fa 137 8c625cfff37cd90c 3 3 824d1c7dad7a86fa
a5 true 0[] 3[1,-1;2,-2;4,4] 1[-3,6]
a6 true true false true true true true true true true true true true task 1[-3,6]
a7 TileGeneratorBatched 5c3c2185ae5ff301 true ff2a52d36fd50b5d true TileGeneratorStriped 5c3c2185ae5ff301 true ff2a52d36fd50b5d true
a8 8 true 1096 c8fc16f9a1adfa38
";
}
