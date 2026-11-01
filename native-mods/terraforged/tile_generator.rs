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
    terraforged_concurrent::{Cache, CacheEntry, LazyCallable},
    terraforged_engine::Cell,
    terraforged_filters::WorldFilters,
    terraforged_river::Rivermap,
    terraforged_tile::{GenChunk, Tile},
};
use std::sync::Arc;

pub trait HeightmapSource {
    fn apply_base(&self, cell: &mut Cell, x: f32, z: f32);

    fn rivermap(&self, x: i32, z: i32) -> Arc<Rivermap>;

    fn apply_rivers(&self, cell: &mut Cell, x: f32, z: f32, rivermap: &Rivermap);

    fn apply_climate(&self, cell: &mut Cell, x: f32, z: f32);
}

pub trait TileFilters {
    fn apply(&self, tile: &mut Tile, optional: bool);
}

impl TileFilters for WorldFilters {
    fn apply(&self, tile: &mut Tile, optional: bool) {
        self.apply_tile(tile, optional);
    }
}

pub struct WorldGenerator<H, F> {
    heightmap: H,
    filters: F,
}

impl<H, F> WorldGenerator<H, F> {
    pub const fn new(heightmap: H, filters: F) -> Self {
        Self { heightmap, filters }
    }

    pub const fn heightmap(&self) -> &H {
        &self.heightmap
    }

    pub const fn filters(&self) -> &F {
        &self.filters
    }
}

pub struct TileGenerator<H, F> {
    factor: i32,
    border: i32,
    batch_size: i32,
    style: GenerationStyle,
    generator: WorldGenerator<H, F>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum GenerationStyle {
    Direct,
    Batched,
    Striped,
}

pub struct TileGeneratorBuilder<H, F> {
    factor: i32,
    border: i32,
    batch_size: i32,
    striped: bool,
    generator: Option<WorldGenerator<H, F>>,
}

pub struct TileCache<H, F> {
    can_queue: bool,
    generator: Arc<TileGenerator<H, F>>,
    cache: Cache<CacheEntry<Tile>>,
}

pub struct CachedChunk {
    tile: Arc<CacheEntry<Tile>>,
    chunk_x: i32,
    chunk_z: i32,
    block_x: i32,
    block_z: i32,
}

impl CachedChunk {
    fn new(tile: Arc<CacheEntry<Tile>>, chunk_x: i32, chunk_z: i32) -> Self {
        Self {
            tile,
            chunk_x,
            chunk_z,
            block_x: chunk_x.wrapping_shl(4),
            block_z: chunk_z.wrapping_shl(4),
        }
    }

    pub const fn chunk_x(&self) -> i32 {
        self.chunk_x
    }

    pub const fn chunk_z(&self) -> i32 {
        self.chunk_z
    }

    pub const fn block_x(&self) -> i32 {
        self.block_x
    }

    pub const fn block_z(&self) -> i32 {
        self.block_z
    }

    pub fn cell(&self, block_x: i32, block_z: i32) -> &Cell {
        self.tile.get().cell(
            self.block_x.wrapping_add(block_x & 15),
            self.block_z.wrapping_add(block_z & 15),
        )
    }

    pub fn iterate(&self, mut visitor: impl FnMut(&Cell, i32, i32)) {
        for delta_z in 0..16 {
            for delta_x in 0..16 {
                visitor(self.cell(delta_x, delta_z), delta_x, delta_z);
            }
        }
    }

    pub fn visit(
        &self,
        mut min_x: i32,
        mut min_z: i32,
        mut max_x: i32,
        mut max_z: i32,
        mut visitor: impl FnMut(&Cell, i32, i32),
    ) {
        if max_x < self.block_x || max_z < self.block_z {
            return;
        }
        let region_max_x = self.block_x.wrapping_add(15);
        let region_max_z = self.block_z.wrapping_add(15);
        if min_x > region_max_x || max_z > region_max_z {
            return;
        }
        min_x = min_x.max(self.block_x);
        min_z = min_z.max(self.block_z);
        max_x = max_x.min(region_max_x);
        max_z = max_z.min(region_max_z);
        for z in min_z..=max_x {
            for x in min_x..=max_z {
                visitor(self.cell(x, z), x, z);
            }
        }
    }
}

impl<H, F> TileCache<H, F>
where
    H: HeightmapSource + Send + Sync + 'static,
    F: TileFilters + Send + Sync + 'static,
{
    pub const QUEUING_MIN_POOL_SIZE: usize = 4;
    const CAPACITY: usize = 256;
    const LIFETIME_MILLIS: u64 = 60_000;

    pub fn new(generator: Arc<TileGenerator<H, F>>, worker_count: usize) -> Self {
        Self {
            can_queue: worker_count > Self::QUEUING_MIN_POOL_SIZE,
            generator,
            cache: Cache::new("TileCache", Self::CAPACITY, Self::LIFETIME_MILLIS),
        }
    }

    pub fn chunk_to_region(&self, coordinate: i32) -> i32 {
        self.generator.chunk_to_region(coordinate)
    }

    pub fn get(&self, id: i64) -> Option<Arc<CacheEntry<Tile>>> {
        self.cache.get(id)
    }

    pub fn get_or_compute(&self, id: i64) -> Arc<CacheEntry<Tile>> {
        self.cache.compute_if_absent(id, |id| {
            let region_x = Tile::region_x_from(id);
            let region_z = Tile::region_z_from(id);
            let generator = Arc::clone(&self.generator);
            CacheEntry::compute_async(move || generator.generate_region(region_x, region_z))
        })
    }

    pub fn get_tile(&self, id: i64) -> Arc<CacheEntry<Tile>> {
        let entry = self.get_or_compute(id);
        entry.get();
        entry
    }

    pub fn get_tile_at(&self, region_x: i32, region_z: i32) -> Arc<CacheEntry<Tile>> {
        self.get_tile(Tile::region_id_for(region_x, region_z))
    }

    pub fn get_tile_if_present(&self, id: i64) -> Option<Arc<CacheEntry<Tile>>> {
        let entry = self.get(id)?;
        if entry.is_done() { Some(entry) } else { None }
    }

    pub fn queue_chunk(&self, chunk_x: i32, chunk_z: i32) {
        if self.can_queue {
            self.queue_region(self.chunk_to_region(chunk_x), self.chunk_to_region(chunk_z));
        }
    }

    pub fn queue_region(&self, region_x: i32, region_z: i32) {
        if self.can_queue {
            self.get_or_compute(Tile::region_id_for(region_x, region_z));
        }
    }

    pub fn expire(&self) -> usize {
        self.cache.run()
    }

    pub fn get_chunk(&self, chunk_x: i32, chunk_z: i32) -> CachedChunk {
        let region_x = self.chunk_to_region(chunk_x);
        let region_z = self.chunk_to_region(chunk_z);
        CachedChunk::new(self.get_tile_at(region_x, region_z), chunk_x, chunk_z)
    }
}

impl<H, F> Default for TileGeneratorBuilder<H, F> {
    fn default() -> Self {
        Self {
            factor: 0,
            border: 0,
            batch_size: 0,
            striped: false,
            generator: None,
        }
    }
}

impl<H, F> TileGeneratorBuilder<H, F>
where
    H: HeightmapSource,
    F: TileFilters,
{
    pub fn size(mut self, factor: i32, border: i32) -> Self {
        self.factor = factor;
        self.border = border;
        self
    }

    pub fn factor(mut self, factor: i32) -> Self {
        self.factor = factor;
        self
    }

    pub fn border(mut self, border: i32) -> Self {
        self.border = border;
        self
    }

    pub fn generator(mut self, generator: WorldGenerator<H, F>) -> Self {
        self.generator = Some(generator);
        self
    }

    pub fn batch(mut self, batch_size: i32) -> Self {
        self.batch_size = batch_size;
        self
    }

    pub fn striped(mut self) -> Self {
        self.striped = true;
        self
    }

    pub fn build(self) -> TileGenerator<H, F> {
        let style = if self.batch_size > 0 {
            if self.striped {
                GenerationStyle::Striped
            } else {
                GenerationStyle::Batched
            }
        } else {
            GenerationStyle::Direct
        };
        TileGenerator::with_style(
            self.factor,
            self.border,
            self.batch_size,
            style,
            self.generator.expect("tile generator requires a generator"),
        )
    }
}

impl<H, F> TileGenerator<H, F>
where
    H: HeightmapSource,
    F: TileFilters,
{
    pub const fn new(
        factor: i32,
        border: i32,
        batch_size: i32,
        generator: WorldGenerator<H, F>,
    ) -> Self {
        Self::with_style(
            factor,
            border,
            batch_size,
            GenerationStyle::Direct,
            generator,
        )
    }

    const fn with_style(
        factor: i32,
        border: i32,
        batch_size: i32,
        style: GenerationStyle,
        generator: WorldGenerator<H, F>,
    ) -> Self {
        Self {
            factor,
            border,
            batch_size,
            style,
            generator,
        }
    }

    pub fn builder() -> TileGeneratorBuilder<H, F> {
        TileGeneratorBuilder::default()
    }

    pub const fn generator(&self) -> &WorldGenerator<H, F> {
        &self.generator
    }

    pub const fn batch_size(&self) -> i32 {
        self.batch_size
    }

    pub const fn chunk_to_region(&self, chunk: i32) -> i32 {
        chunk >> self.factor
    }

    pub fn get_tile(
        &self,
        region_x: i32,
        region_z: i32,
    ) -> LazyCallable<Tile, impl Fn() -> Tile + '_> {
        LazyCallable::new(move || self.generate_region(region_x, region_z))
    }

    pub fn get_zoomed_tile(
        &self,
        center_x: f32,
        center_z: f32,
        zoom: f32,
        filter: bool,
    ) -> LazyCallable<Tile, impl Fn() -> Tile + '_> {
        LazyCallable::new(move || self.generate_zoomed_region(center_x, center_z, zoom, filter))
    }

    pub fn generate_region(&self, region_x: i32, region_z: i32) -> Tile {
        let mut tile = self.create_empty_region(region_x, region_z);
        match self.style {
            GenerationStyle::Direct => self.generate_heightmap(&mut tile),
            GenerationStyle::Batched => {
                self.generate_area(&mut tile, self.batch_size, SampleTransform::Direct)
            }
            GenerationStyle::Striped => {
                self.generate_striped(&mut tile, self.batch_size, SampleTransform::Direct)
            }
        }
        self.post_process(&mut tile, true);
        tile
    }

    pub fn generate_zoomed_region(
        &self,
        center_x: f32,
        center_z: f32,
        zoom: f32,
        filter: bool,
    ) -> Tile {
        let mut tile = self.create_empty_region(0, 0);
        match self.style {
            GenerationStyle::Direct => {
                self.generate_zoomed_heightmap(&mut tile, center_x, center_z, zoom)
            }
            GenerationStyle::Batched => {
                let transform = SampleTransform::zoomed(&tile, center_x, center_z, zoom);
                self.generate_area(&mut tile, self.batch_size, transform);
            }
            GenerationStyle::Striped => {
                let transform = SampleTransform::zoomed(&tile, center_x, center_z, zoom);
                self.generate_striped(&mut tile, self.batch_size, transform);
            }
        }
        self.post_process(&mut tile, filter);
        tile
    }

    pub fn generate_batched_region(&self, region_x: i32, region_z: i32, batch_count: i32) -> Tile {
        let mut tile = self.create_empty_region(region_x, region_z);
        self.generate_area(&mut tile, batch_count, SampleTransform::Direct);
        self.post_process(&mut tile, true);
        tile
    }

    pub fn generate_batched_zoomed_region(
        &self,
        center_x: f32,
        center_z: f32,
        zoom: f32,
        filter: bool,
        batch_count: i32,
    ) -> Tile {
        let mut tile = self.create_empty_region(0, 0);
        let transform = SampleTransform::zoomed(&tile, center_x, center_z, zoom);
        self.generate_area(&mut tile, batch_count, transform);
        self.post_process(&mut tile, filter);
        tile
    }

    pub fn generate_striped_region(&self, region_x: i32, region_z: i32, sections: i32) -> Tile {
        let mut tile = self.create_empty_region(region_x, region_z);
        self.generate_striped(&mut tile, sections, SampleTransform::Direct);
        self.post_process(&mut tile, true);
        tile
    }

    pub fn generate_striped_zoomed_region(
        &self,
        center_x: f32,
        center_z: f32,
        zoom: f32,
        filter: bool,
        sections: i32,
    ) -> Tile {
        let mut tile = self.create_empty_region(0, 0);
        let transform = SampleTransform::zoomed(&tile, center_x, center_z, zoom);
        self.generate_striped(&mut tile, sections, transform);
        self.post_process(&mut tile, filter);
        tile
    }

    pub fn create_empty_region(&self, region_x: i32, region_z: i32) -> Tile {
        Tile::new(region_x, region_z, self.factor, self.border)
    }

    fn generate_heightmap(&self, tile: &mut Tile) {
        let heightmap = self.generator.heightmap();
        tile.generate_heightmap(
            |cell, x, z| heightmap.apply_base(cell, x, z),
            |x, z| heightmap.rivermap(x, z),
            |cell, x, z, map| heightmap.apply_rivers(cell, x, z, map),
            |cell, x, z| heightmap.apply_climate(cell, x, z),
        );
    }

    fn generate_zoomed_heightmap(&self, tile: &mut Tile, center_x: f32, center_z: f32, zoom: f32) {
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
    }

    fn post_process(&self, tile: &mut Tile, filter: bool) {
        self.generator.filters().apply(tile, filter);
    }

    fn generate_area(&self, tile: &mut Tile, batch_count: i32, transform: SampleTransform) {
        let chunk_size = tile.chunk_size();
        let batch_size = Tile::batch_size(batch_count, chunk_size);
        for delta_z in 0..batch_count {
            let start_z = delta_z.wrapping_mul(batch_size);
            let max_z = chunk_size.total.min(start_z.wrapping_add(batch_size));
            for delta_x in 0..batch_count {
                let start_x = delta_x.wrapping_mul(batch_size);
                let max_x = chunk_size.total.min(start_x.wrapping_add(batch_size));
                for chunk_z in start_z..max_z {
                    for chunk_x in start_x..max_x {
                        generate_chunk(
                            tile.chunk_writer(chunk_x, chunk_z),
                            self.generator.heightmap(),
                            transform,
                        );
                    }
                }
            }
        }
    }

    fn generate_striped(&self, tile: &mut Tile, sections: i32, transform: SampleTransform) {
        let chunk_size = tile.chunk_size();
        let section_length = Tile::batch_size(sections, chunk_size);
        for chunk_z in 0..chunk_size.total {
            for section in 0..sections {
                let start_x = section.wrapping_mul(section_length);
                let max_x = chunk_size.total.min(start_x.wrapping_add(section_length));
                for chunk_x in start_x..max_x {
                    generate_chunk(
                        tile.chunk_writer(chunk_x, chunk_z),
                        self.generator.heightmap(),
                        transform,
                    );
                }
            }
        }
    }
}

impl<H, F> TileGenerator<H, F>
where
    H: HeightmapSource + Send + Sync + 'static,
    F: TileFilters + Send + Sync + 'static,
{
    pub fn cached(self: Arc<Self>, worker_count: usize) -> TileCache<H, F> {
        TileCache::new(self, worker_count)
    }
}

#[derive(Clone, Copy)]
enum SampleTransform {
    Direct,
    Zoom {
        translate_x: f32,
        translate_z: f32,
        zoom: f32,
    },
}

impl SampleTransform {
    fn zoomed(tile: &Tile, center_x: f32, center_z: f32, zoom: f32) -> Self {
        Self::Zoom {
            translate_x: center_x - tile.block_size().size as f32 * zoom / 2.0,
            translate_z: center_z - tile.block_size().size as f32 * zoom / 2.0,
            zoom,
        }
    }

    fn apply(self, x: i32, z: i32) -> (f32, f32) {
        match self {
            Self::Direct => (x as f32, z as f32),
            Self::Zoom {
                translate_x,
                translate_z,
                zoom,
            } => (x as f32 * zoom + translate_x, z as f32 * zoom + translate_z),
        }
    }
}

fn generate_chunk<H: HeightmapSource>(
    mut chunk: GenChunk<'_>,
    heightmap: &H,
    transform: SampleTransform,
) {
    let mut current = None;
    chunk.generate(|cell, _, _, block_x, block_z| {
        let (x, z) = transform.apply(block_x, block_z);
        heightmap.apply_base(cell, x, z);
        let rivermap = Rivermap::get_for_cell(cell, current.take(), |map_x, map_z| {
            heightmap.rivermap(map_x, map_z)
        });
        heightmap.apply_rivers(cell, x, z, &rivermap);
        heightmap.apply_climate(cell, x, z);
        current = Some(rivermap);
    });
}

#[cfg(test)]
mod tests {
    use super::super::{terraforged_engine::TerrainRegistry, terraforged_river::GenWarp};
    use super::*;
    use std::sync::Mutex;

    struct TestHeightmap {
        map: Arc<Rivermap>,
        events: Arc<Mutex<Vec<&'static str>>>,
    }

    impl HeightmapSource for TestHeightmap {
        fn apply_base(&self, cell: &mut Cell, x: f32, z: f32) {
            self.events.lock().unwrap().push("base");
            cell.value = x * 1_000.0 + z;
            cell.continent_x = 7;
            cell.continent_z = -9;
            cell.terrain = TerrainRegistry::new().get_by_id(8);
        }

        fn rivermap(&self, x: i32, z: i32) -> Arc<Rivermap> {
            self.events.lock().unwrap().push("rivermap");
            assert_eq!((x, z), (7, -9));
            Arc::clone(&self.map)
        }

        fn apply_rivers(&self, cell: &mut Cell, _: f32, _: f32, _: &Rivermap) {
            self.events.lock().unwrap().push("rivers");
            cell.river_mask = 0.25;
        }

        fn apply_climate(&self, cell: &mut Cell, _: f32, _: f32) {
            self.events.lock().unwrap().push("climate");
            cell.temperature = 0.75;
        }
    }

    struct TestFilters(Arc<Mutex<Vec<bool>>>);

    impl TileFilters for TestFilters {
        fn apply(&self, tile: &mut Tile, optional: bool) {
            self.0.lock().unwrap().push(optional);
            tile.cell_mut(0, 0).moisture = 0.5;
        }
    }

    #[test]
    fn synchronous_tile_generator_matches_engine_0_3_0() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let filters = Arc::new(Mutex::new(Vec::new()));
        let heightmap = TestHeightmap {
            map: Arc::new(Rivermap::new(7, -9, Vec::new(), GenWarp::direct())),
            events: Arc::clone(&events),
        };
        let generator = TileGenerator::new(
            0,
            0,
            0,
            WorldGenerator::new(heightmap, TestFilters(Arc::clone(&filters))),
        );

        assert_eq!(generator.batch_size(), 0);
        assert_eq!(generator.chunk_to_region(-3), -3);
        let tile = generator.generate_region(-2, 3);
        assert_eq!((tile.region_x(), tile.region_z()), (-2, 3));
        assert_eq!(tile.cell(0, 0).value, -31_952.0);
        assert_eq!(tile.cell(0, 0).moisture, 0.5);
        assert_eq!(*filters.lock().unwrap(), [true]);
        assert_eq!(
            &events.lock().unwrap()[..4],
            ["base", "rivermap", "rivers", "climate"]
        );

        let zoomed = generator.generate_zoomed_region(100.0, -50.0, 0.5, false);
        assert_eq!((zoomed.region_x(), zoomed.region_z()), (0, 0));
        assert_eq!(zoomed.cell(0, 0).value, 95_946.0);
        assert_eq!(*filters.lock().unwrap(), [true, false]);

        let lazy = generator.get_tile(4, -5);
        assert!(!lazy.is_done());
        assert_eq!((lazy.get().region_x(), lazy.get().region_z()), (4, -5));
        assert!(lazy.is_done());
        assert_eq!(*filters.lock().unwrap(), [true, false, true]);

        let zoomed = generator.get_zoomed_tile(25.0, 75.0, 2.0, false);
        assert!(!zoomed.is_done());
        assert_eq!((zoomed.get().region_x(), zoomed.get().region_z()), (0, 0));
        assert_eq!(*filters.lock().unwrap(), [true, false, true, false]);
    }

    #[test]
    fn batched_tile_generators_match_engine_0_3_0() {
        let create = || {
            let events = Arc::new(Mutex::new(Vec::new()));
            let filters = Arc::new(Mutex::new(Vec::new()));
            let heightmap = TestHeightmap {
                map: Arc::new(Rivermap::new(7, -9, Vec::new(), GenWarp::direct())),
                events: Arc::clone(&events),
            };
            (
                TileGenerator::new(
                    1,
                    0,
                    3,
                    WorldGenerator::new(heightmap, TestFilters(filters)),
                ),
                events,
            )
        };

        let (generator, events) = create();
        let batched = generator.generate_batched_region(1, -1, 3);
        assert_eq!(batched.cell(0, 0).value, 31_968.0);
        assert_eq!(batched.cell(31, 31).value, 62_999.0);
        assert_eq!(rivermap_events(&events), 4);

        let (generator, events) = create();
        let striped = generator.generate_striped_region(1, -1, 3);
        assert_eq!(striped.cell(0, 0).value, 31_968.0);
        assert_eq!(striped.cell(31, 31).value, 62_999.0);
        assert_eq!(rivermap_events(&events), 4);

        let (generator, _) = create();
        let batched = generator.generate_batched_zoomed_region(100.0, -50.0, 0.5, false, 3);
        let striped = generator.generate_striped_zoomed_region(100.0, -50.0, 0.5, false, 3);
        assert_eq!(batched.cell(0, 0).value, striped.cell(0, 0).value);
        assert_eq!(batched.cell(31, 31).value, striped.cell(31, 31).value);
    }

    #[test]
    fn tile_generator_builder_matches_engine_0_3_0() {
        let create = || {
            let events = Arc::new(Mutex::new(Vec::new()));
            let heightmap = TestHeightmap {
                map: Arc::new(Rivermap::new(7, -9, Vec::new(), GenWarp::direct())),
                events,
            };
            WorldGenerator::new(heightmap, TestFilters(Arc::new(Mutex::new(Vec::new()))))
        };

        let direct = TileGenerator::builder()
            .size(1, 0)
            .batch(0)
            .striped()
            .generator(create())
            .build();
        assert_eq!(direct.style, GenerationStyle::Direct);

        let batched = TileGenerator::builder()
            .factor(1)
            .border(0)
            .batch(3)
            .generator(create())
            .build();
        assert_eq!(batched.style, GenerationStyle::Batched);
        assert_eq!(batched.generate_region(1, -1).cell(31, 31).value, 62_999.0);

        let striped = TileGenerator::builder()
            .size(1, 0)
            .batch(3)
            .striped()
            .generator(create())
            .build();
        assert_eq!(striped.style, GenerationStyle::Striped);
        assert_eq!(striped.generate_region(1, -1).cell(31, 31).value, 62_999.0);
    }

    #[test]
    fn tile_cache_matches_engine_0_3_0() {
        let filters = Arc::new(Mutex::new(Vec::new()));
        let heightmap = TestHeightmap {
            map: Arc::new(Rivermap::new(7, -9, Vec::new(), GenWarp::direct())),
            events: Arc::new(Mutex::new(Vec::new())),
        };
        let generator = Arc::new(TileGenerator::new(
            1,
            0,
            0,
            WorldGenerator::new(heightmap, TestFilters(Arc::clone(&filters))),
        ));
        let cache = Arc::clone(&generator).cached(5);

        cache.queue_region(2, -3);
        let id = Tile::region_id_for(2, -3);
        let queued = cache.get(id).unwrap();
        while !queued.is_done() {
            std::thread::yield_now();
        }
        assert_eq!((queued.get().region_x(), queued.get().region_z()), (2, -3));
        assert!(Arc::ptr_eq(&queued, &cache.get_or_compute(id)));
        assert_eq!(filters.lock().unwrap().len(), 1);

        cache.queue_chunk(8, -12);
        assert!(cache.get(Tile::region_id_for(4, -6)).is_some());
        let direct = cache.get_tile_at(-1, 3);
        assert_eq!((direct.get().region_x(), direct.get().region_z()), (-1, 3));

        let disabled = generator.cached(4);
        disabled.queue_region(9, 9);
        assert!(disabled.get(Tile::region_id_for(9, 9)).is_none());
    }

    #[test]
    fn cached_chunk_reader_matches_engine_0_3_0() {
        let heightmap = TestHeightmap {
            map: Arc::new(Rivermap::new(7, -9, Vec::new(), GenWarp::direct())),
            events: Arc::new(Mutex::new(Vec::new())),
        };
        let generator = Arc::new(TileGenerator::new(
            1,
            0,
            0,
            WorldGenerator::new(heightmap, TestFilters(Arc::new(Mutex::new(Vec::new())))),
        ));
        let cache = generator.cached(5);
        let chunk = cache.get_chunk(5, -11);

        assert_eq!(
            (
                chunk.chunk_x(),
                chunk.chunk_z(),
                chunk.block_x(),
                chunk.block_z(),
            ),
            (5, -11, 80, -176)
        );
        assert_eq!(chunk.cell(0, 0).value, 79_824.0);
        assert_eq!(chunk.cell(15, 15).value, 94_839.0);
        let mut count = 0;
        chunk.iterate(|_, _, _| count += 1);
        assert_eq!(count, 256);
    }

    fn rivermap_events(events: &Mutex<Vec<&'static str>>) -> usize {
        events
            .lock()
            .unwrap()
            .iter()
            .filter(|event| **event == "rivermap")
            .count()
    }
}
