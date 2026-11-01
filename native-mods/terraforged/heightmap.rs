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
    terraforged_engine::{Cell, Levels, TerrainRegistry},
    terraforged_populator::VolcanoPopulator,
    terraforged_river::Rivermap,
    terraforged_util::pack,
};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

pub struct HeightmapRivers {
    levels: Levels,
    terrains: TerrainRegistry,
}

impl HeightmapRivers {
    pub fn new(levels: Levels, terrains: TerrainRegistry) -> Self {
        Self { levels, terrains }
    }

    pub fn apply(&self, cell: &mut Cell, x: f32, z: f32, rivermap: &Rivermap) {
        rivermap.apply(cell, x, z);
        VolcanoPopulator::modify_volcano_type(cell, self.levels, &self.terrains);
    }
}

pub struct HeightmapCache {
    water_level: f32,
    beach_level: f32,
    beach: super::terraforged_engine::Terrain,
    capacity: usize,
    cells: Mutex<HashMap<i64, Cell>>,
}

impl HeightmapCache {
    pub const CACHE_SIZE: usize = 4_096;

    pub fn new(levels: Levels, terrains: &TerrainRegistry) -> Self {
        Self::with_capacity(levels, terrains, Self::CACHE_SIZE)
    }

    pub fn with_capacity(levels: Levels, terrains: &TerrainRegistry, capacity: usize) -> Self {
        Self {
            water_level: levels.water,
            beach_level: levels.water(5),
            beach: terrains.get_by_id(4),
            capacity,
            cells: Mutex::new(HashMap::with_capacity(capacity)),
        }
    }

    pub fn get(&self, x: i32, z: i32, apply: impl FnOnce(&mut Cell, f32, f32)) -> Cell {
        let id = pack(x, z);
        let mut cells = self.cells.lock().unwrap_or_else(|error| error.into_inner());
        if let Some(cell) = cells.get(&id) {
            return cell.clone();
        }
        if cells.len() >= self.capacity {
            cells.clear();
        }
        let mut cell = Cell::default();
        apply(&mut cell, x as f32, z as f32);
        if cell.terrain.id() == 3 && cell.value > self.water_level && cell.value <= self.beach_level
        {
            cell.terrain = self.beach.clone();
        }
        cells.insert(id, cell.clone());
        cell
    }

    #[allow(clippy::too_many_arguments)]
    pub fn generate(
        &self,
        cell: &mut Cell,
        x: i32,
        z: i32,
        current: Option<Arc<Rivermap>>,
        apply_base: impl FnOnce(&mut Cell, f32, f32),
        lookup: impl FnOnce(i32, i32) -> Arc<Rivermap>,
        apply_rivers: impl FnOnce(&mut Cell, f32, f32, &Rivermap),
        apply_climate: impl FnOnce(&mut Cell, f32, f32),
    ) -> Option<Arc<Rivermap>> {
        let id = pack(x, z);
        let mut cells = self.cells.lock().unwrap_or_else(|error| error.into_inner());
        if let Some(cached) = cells.get(&id) {
            cell.copy_from(cached);
            return current;
        }
        if cells.len() >= self.capacity {
            cells.clear();
        }
        let sample_x = x as f32;
        let sample_z = z as f32;
        apply_base(cell, sample_x, sample_z);
        let rivermap = Rivermap::get_for_cell(cell, current, lookup);
        apply_rivers(cell, sample_x, sample_z, &rivermap);
        apply_climate(cell, sample_x, sample_z);
        cells.insert(id, cell.clone());
        Some(rivermap)
    }
}

#[cfg(test)]
mod tests {
    use super::super::terraforged_river::GenWarp;
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn heightmap_rivers_match_engine_0_3_0() {
        let terrains = TerrainRegistry::new();
        let stage = HeightmapRivers::new(Levels::new(256, 63), TerrainRegistry::new());
        let map = Rivermap::new(17, -9, Vec::new(), GenWarp::direct());

        for (value, river_mask, terrain) in [
            (0.1, 0.5, 14),
            (0.1, 1.0, 14),
            (0.5, 0.5, 14),
            (0.5, 1.0, 15),
        ] {
            let mut cell = Cell::default();
            cell.terrain = terrains.get_by_id(15);
            cell.value = value;
            cell.river_mask = river_mask;
            stage.apply(&mut cell, 12.25, -98.75, &map);
            assert_eq!(cell.terrain.id(), terrain);
        }
    }

    #[test]
    fn heightmap_cache_matches_engine_0_3_0() {
        let terrains = TerrainRegistry::new();
        let levels = Levels::new(256, 63);
        let cache = HeightmapCache::with_capacity(levels, &terrains, 16);
        let calls = AtomicUsize::new(0);
        let first = cache.get(12, -34, |cell, _, _| {
            calls.fetch_add(1, Ordering::Relaxed);
            cell.terrain = terrains.get_by_id(3);
            cell.value = levels.water(3);
        });
        let second = cache.get(12, -34, |_, _, _| unreachable!());
        assert_eq!(calls.load(Ordering::Relaxed), 1);
        assert_eq!((first.terrain.id(), second.terrain.id()), (4, 4));
        assert_eq!(first.value.to_bits(), second.value.to_bits());

        let coast = cache.get(13, -34, |cell, _, _| {
            cell.terrain = terrains.get_by_id(3);
            cell.value = levels.water(6);
        });
        assert_eq!(coast.terrain.id(), 3);
    }

    #[test]
    fn contextual_heightmap_cache_matches_engine_0_3_0() {
        let terrains = TerrainRegistry::new();
        let cache = HeightmapCache::with_capacity(Levels::new(256, 63), &terrains, 16);
        let map = Arc::new(Rivermap::new(17, -9, Vec::new(), GenWarp::direct()));
        let order = Mutex::new(Vec::new());
        let mut cell = Cell::default();
        let selected = cache.generate(
            &mut cell,
            12,
            -34,
            Some(Arc::clone(&map)),
            |cell, _, _| {
                order.lock().unwrap().push("base");
                cell.continent_x = 17;
                cell.continent_z = -9;
                cell.value = 0.6;
            },
            |_, _| unreachable!(),
            |cell, _, _, _| {
                order.lock().unwrap().push("rivers");
                cell.river_mask = 0.25;
            },
            |cell, _, _| {
                order.lock().unwrap().push("climate");
                cell.temperature = 0.75;
            },
        );
        assert!(Arc::ptr_eq(&map, selected.as_ref().unwrap()));
        assert_eq!(*order.lock().unwrap(), ["base", "rivers", "climate"]);

        let mut cached = Cell::default();
        let reused = cache.generate(
            &mut cached,
            12,
            -34,
            selected,
            |_, _, _| unreachable!(),
            |_, _| unreachable!(),
            |_, _, _, _| unreachable!(),
            |_, _, _| unreachable!(),
        );
        assert!(Arc::ptr_eq(&map, reused.as_ref().unwrap()));
        assert_eq!(cached.river_mask.to_bits(), 0x3e800000);
        assert_eq!(cached.temperature.to_bits(), 0x3f400000);
    }
}
