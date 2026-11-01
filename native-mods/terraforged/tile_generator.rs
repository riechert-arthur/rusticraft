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
    terraforged_concurrent::LazyCallable, terraforged_engine::Cell,
    terraforged_filters::WorldFilters, terraforged_river::Rivermap, terraforged_tile::Tile,
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
    generator: WorldGenerator<H, F>,
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
        Self {
            factor,
            border,
            batch_size,
            generator,
        }
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
        self.generate_heightmap(&mut tile);
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
        self.generate_zoomed_heightmap(&mut tile, center_x, center_z, zoom);
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
}
