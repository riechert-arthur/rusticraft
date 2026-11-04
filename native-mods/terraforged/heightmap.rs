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
    terraforged_engine::{Cell, Levels, Terrain, TerrainRegistry},
    terraforged_populator::VolcanoPopulator,
    terraforged_river::Rivermap,
    terraforged_util::pack,
};
use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
};

/// The river stage of Engine 0.3.0's `Heightmap` (`applyRivers`): it carves a continent's river map into a cell,
/// then turns volcano pipes that ended up under water or in a river back into volcanoes.
pub struct HeightmapRivers {
    levels: Levels,
    terrains: Arc<TerrainRegistry>,
}

impl HeightmapRivers {
    /// `levels` are the context's (`Heightmap.levels`); `terrains` is the world's registry, whose built-ins the
    /// volcano check compares the cell's terrain with.
    pub fn new(levels: Levels, terrains: Arc<TerrainRegistry>) -> Self {
        Self { levels, terrains }
    }

    /// `applyRivers(cell, x, z, rivermap)`: `rivermap.apply`, then `VolcanoPopulator.modifyVolcanoType`.
    pub fn apply(&self, cell: &mut Cell, x: f32, z: f32, rivermap: &Rivermap) {
        rivermap.apply(cell, x, z);
        VolcanoPopulator::modify_volcano_type(cell, self.levels, &self.terrains);
    }

    /// `applyRivers(cell, x, z)`, the form `Heightmap.apply` runs: the river map of the cell's continent cell
    /// (`Continent.getRivermap(cell)`, i.e. `rivermap(cell.continentX, cell.continentZ)`, e.g. a continent
    /// generator's `rivermap`), then [`Self::apply`].
    pub fn apply_continent(
        &self,
        cell: &mut Cell,
        x: f32,
        z: f32,
        rivermap: impl FnOnce(i32, i32) -> Arc<Rivermap>,
    ) {
        let rivermap = rivermap(cell.continent_x, cell.continent_z);
        self.apply(cell, x, z, &rivermap);
    }
}

/// Engine 0.3.0's `HeightmapCache.get(x, z)`: memoizes the heightmap's cell of each block (keyed by
/// `PosUtil.pack(x, z)`) and, on a miss, turns a coast cell whose value is above the water level and at most five
/// blocks over it into a beach (the same correction as `WorldLookup.compute`).
///
/// `Heightmap` itself is not ported yet, so [`Self::get`] takes its `apply(cell, x, z)` as a closure. Engine's
/// `get` returns the shared cached `Cell` object; this returns a copy (every field, as `Clone`), so callers cannot
/// change what the cache holds. Engine's `generate(cell, x, z, rivermap)` (the tile path, which caches the
/// caller's own cell and does not apply the beach correction) is not ported.
///
/// Engine's `LoadBalanceLongMap` splits the cache into per-thread-count sections, each dropping its oldest entry
/// when full, and computes misses under a section's write lock. Here one map drops its oldest entry once it holds
/// `capacity` cells, and misses are computed outside the lock, so threads don't wait on each other's heightmap
/// work (two threads missing the same block may both compute it; the cells are equal). Which cells stay cached
/// never changes what `get` returns.
pub struct HeightmapCache {
    water_level: f32,
    beach_level: f32,
    terrains: Arc<TerrainRegistry>,
    coast: Terrain,
    beach: Terrain,
    capacity: usize,
    cells: Mutex<Cells>,
}

/// The cached cells and their keys, oldest first.
struct Cells {
    map: HashMap<i64, Cell>,
    order: VecDeque<i64>,
}

impl HeightmapCache {
    /// Engine's `HeightmapCache.CACHE_SIZE`.
    pub const CACHE_SIZE: usize = 4_096;

    /// `new HeightmapCache(heightmap)`: `levels` are the heightmap's (`heightmap.getLevels()`), `terrains` the
    /// world's registry, which the cells start from and whose `coast` and `beach` the correction uses.
    pub fn new(levels: Levels, terrains: Arc<TerrainRegistry>) -> Self {
        Self::with_capacity(levels, terrains, Self::CACHE_SIZE)
    }

    /// `new HeightmapCache(heightmap, size)`.
    pub fn with_capacity(levels: Levels, terrains: Arc<TerrainRegistry>, capacity: usize) -> Self {
        Self {
            water_level: levels.water,
            beach_level: levels.water(5),
            coast: terrains.get_by_id(3),
            beach: terrains.get_by_id(4),
            terrains,
            capacity,
            cells: Mutex::new(Cells {
                map: HashMap::with_capacity(capacity),
                order: VecDeque::with_capacity(capacity),
            }),
        }
    }

    /// `get(x, z)`: the cached cell of block (x, z), or on a miss a new cell given `apply(cell, x, z)` (Engine's
    /// `heightmap.apply`, with the block coordinates as floats) and the beach correction.
    pub fn get(&self, x: i32, z: i32, apply: impl FnOnce(&mut Cell, f32, f32)) -> Cell {
        let id = pack(x, z);
        if let Some(cell) = self.lock().map.get(&id) {
            return cell.clone();
        }
        let mut cell = Cell::new(&self.terrains);
        apply(&mut cell, x as f32, z as f32);
        // `cell.terrain == TerrainType.COAST`: by identity, against the world registry's coast.
        if cell.terrain.is_same(&self.coast)
            && cell.value > self.water_level
            && cell.value <= self.beach_level
        {
            cell.terrain = self.beach.clone();
        }
        let mut cells = self.lock();
        if !cells.map.contains_key(&id) {
            // A capacity of 0 keeps one cell (Engine's map throws on its first miss).
            while cells.map.len() >= self.capacity.max(1) {
                let Some(oldest) = cells.order.pop_front() else {
                    break;
                };
                cells.map.remove(&oldest);
            }
            cells.map.insert(id, cell.clone());
            cells.order.push_back(id);
        }
        cell
    }

    /// The cells. `apply` runs outside the lock, so a poisoned lock (a panic while inserting) still guards a
    /// consistent map.
    fn lock(&self) -> std::sync::MutexGuard<'_, Cells> {
        self.cells.lock().unwrap_or_else(|error| error.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::super::terraforged_advanced_continent::AdvancedContinentGenerator;
    use super::super::terraforged_continent::SimpleContinentGenerator;
    use super::super::terraforged_engine::{RiverSettings, Seed, WorldSettings};
    use super::super::terraforged_fancy::FancyContinentGenerator;
    use super::super::terraforged_populator::Populator;
    use super::super::terraforged_river::GenWarp;
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    const NAN: f32 = f32::NAN;

    /// The pipe (15) is demoted to a volcano (14) below the water level or with a river mask under 0.85.
    #[test]
    fn demotes_submerged_or_river_cut_pipes() {
        let terrains = Arc::new(TerrainRegistry::new());
        let stage = HeightmapRivers::new(Levels::new(256, 63), Arc::clone(&terrains));
        let map = Rivermap::new(17, -9, Vec::new(), GenWarp::direct());

        for (value, river_mask, terrain) in [
            (0.1, 0.5, 14),
            (0.1, 1.0, 14),
            (0.5, 0.5, 14),
            (0.5, 0.849_999_9, 14),
            (0.5, 0.85, 15),
            (0.5, 1.0, 15),
            (NAN, 1.0, 15),
            (0.5, NAN, 15),
        ] {
            let mut cell = Cell::new(&terrains);
            cell.terrain = terrains.get_by_id(15);
            cell.value = value;
            cell.river_mask = river_mask;
            stage.apply(&mut cell, 12.25, -98.75, &map);
            assert!(cell.terrain.is_same(&terrains.get_by_id(terrain)));
        }
    }

    /// `SimpleRiverGeneratorOracle.settings` (kinds 0 to 2) and `FancyRiverGeneratorOracle.settings` (kind 3).
    fn settings(kind: i32, variant: i32) -> (WorldSettings, RiverSettings) {
        let mut world = WorldSettings {
            seed: 12_345,
            ..WorldSettings::default()
        };
        let mut rivers = RiverSettings::default();
        let simple = kind < 3;
        match variant {
            1 => {
                world.seed = -987_654_321;
                rivers.seed_offset = i32::MAX;
                rivers.lakes.chance = 1.0;
                rivers.wetlands.chance = 1.0;
                if simple {
                    rivers.river_count = 3;
                }
            }
            2 => {
                world.properties.world_height = 384;
                world.properties.sea_level = 62;
                world.control_points.inland = 0.75;
                if simple {
                    rivers.lakes.chance = 0.0;
                }
            }
            3 if simple => {
                world.continent.continent_scale = 1000;
                rivers.river_count = 5;
                rivers.lakes.chance = 0.5;
            }
            3 => world.continent.continent_scale = 1000,
            4 if simple => {
                rivers.river_count = 1;
                world.seed = 7;
            }
            4 => {
                world.continent.continent_scale = 777;
                rivers.lakes.chance = 0.5;
                world.seed = 7;
            }
            _ => {}
        }
        (world, rivers)
    }

    enum Continent {
        Simple(SimpleContinentGenerator),
        Advanced(AdvancedContinentGenerator),
        Fancy(FancyContinentGenerator),
    }

    impl Continent {
        /// The multi (0), single (1), advanced (2) or fancy (3) continent generator, built like
        /// `new ...ContinentGenerator(new Seed(continent_seed), GeneratorContext.createNoCache(settings))`.
        fn new(kind: i32, variant: i32, registry: &Arc<TerrainRegistry>) -> (Self, Levels) {
            let (world, rivers) = settings(kind, variant);
            let levels = Levels::new(world.properties.world_height, world.properties.sea_level);
            let seed = &mut Seed::new(-98_765);
            let root = world.seed as i32;
            let registry = Arc::clone(registry);
            let continent = match kind {
                0 => Self::Simple(SimpleContinentGenerator::multi(
                    seed, root, world, rivers, levels, registry,
                )),
                1 => Self::Simple(SimpleContinentGenerator::single(
                    seed, root, world, rivers, levels, registry,
                )),
                2 => Self::Advanced(AdvancedContinentGenerator::new(
                    seed, root, world, rivers, levels, registry,
                )),
                _ => Self::Fancy(FancyContinentGenerator::new(
                    seed, root, world, rivers, levels, registry,
                )),
            };
            (continent, levels)
        }

        fn populator(&self) -> &dyn Populator {
            match self {
                Self::Simple(continent) => continent,
                Self::Advanced(continent) => continent,
                Self::Fancy(continent) => continent,
            }
        }

        fn rivermap(&self, x: i32, z: i32) -> Arc<Rivermap> {
            match self {
                Self::Simple(continent) => continent.rivermap(x, z),
                Self::Advanced(continent) => continent.rivermap(x, z),
                Self::Fancy(continent) => continent.rivermap(x, z),
            }
        }
    }

    struct Fold(i32);

    impl Fold {
        fn mix(&mut self, v: i32) {
            self.0 = self.0.wrapping_mul(31).wrapping_add(v);
        }
        /// `Float.floatToIntBits`: every NaN folds as 0x7fc00000.
        fn f(&mut self, v: f32) {
            self.mix(if v.is_nan() {
                0x7fc0_0000
            } else {
                v.to_bits() as i32
            });
        }
        fn cell(&mut self, c: &Cell, registry: &TerrainRegistry) {
            for v in [
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
            ] {
                self.f(v);
            }
            self.mix(c.terrain_region_center as i32);
            self.mix((c.terrain_region_center as u64 >> 32) as i32);
            self.mix(c.continent_x);
            self.mix(c.continent_z);
            self.mix(i32::from(c.erosion_mask));
            // Engine compares terrains by identity: the cell's must be the world registry's own.
            assert!(c.terrain.is_same(&registry.get_by_id(c.terrain.id())));
            self.mix(c.terrain.id());
            self.mix(c.biome as i32);
        }
    }

    /// Engine 0.3.0's `Heightmap.applyRivers(cell, x, z, continent.getRivermap(cell))` against
    /// `HeightmapRiversOracle.java` (in the maintainer's `.local/terraforged/oracles/heightmap/`): the multi, single,
    /// advanced and fancy continents for five settings variants each, on a block grid plus odd points, every cell
    /// first given the continent's own `apply` and then a fill of value, river mask and terrain (or none).
    #[test]
    fn applies_continent_rivermaps_like_engine() {
        const FILLS: [(f32, f32, i32); 6] = [
            (0.5, 1.0, 15),
            (0.1, 1.0, 15),
            (0.5, 0.9, 15),
            (NAN, 1.0, 15),
            (0.6, 1.0, 14),
            (0.4, 1.0, 8),
        ];
        let mut points = Vec::new();
        for i in 0..=60 {
            for j in 0..=60 {
                points.push((-3000.0 + 97.0 * i as f32, -2500.0 + 89.0 * j as f32));
            }
        }
        for e in [-0.0, NAN, f32::INFINITY, 1e20] {
            points.push((e, 0.0));
            points.push((700.0, e));
        }
        let registry = Arc::new(TerrainRegistry::new());
        let pipe = registry.get_by_id(15);
        let volcano = registry.get_by_id(14);
        let mut fold = Fold(17);
        let (mut applies, mut demoted, mut carved) = (0, 0, 0);
        for kind in 0..4 {
            for variant in 0..5 {
                let (continent, levels) = Continent::new(kind, variant, &registry);
                let stage = HeightmapRivers::new(levels, Arc::clone(&registry));
                for &(x, z) in &points {
                    for i in std::iter::once(None).chain((0..FILLS.len()).map(Some)) {
                        let mut cell = Cell::new(&registry);
                        continent.populator().apply(&mut cell, x, z);
                        if let Some(i) = i {
                            let (value, river_mask, terrain) = FILLS[i];
                            cell.value = value;
                            cell.river_mask = river_mask;
                            cell.terrain = registry.get_by_id(terrain);
                        }
                        let was_pipe = cell.terrain.is_same(&pipe);
                        let mask = cell.river_mask;
                        stage.apply_continent(&mut cell, x, z, |x, z| continent.rivermap(x, z));
                        demoted += i32::from(was_pipe && cell.terrain.is_same(&volcano));
                        // Java's `!=`: a NaN mask counts as changed.
                        carved += i32::from(cell.river_mask != mask);
                        fold.cell(&cell, &registry);
                        applies += 1;
                    }
                }
            }
        }
        assert_eq!(
            (applies, demoted, carved, fold.0),
            (522_060, 110_747, 124_079, -1_473_748_014)
        );
    }

    /// A hit returns the first lookup's cell without calling `apply` again; only the world registry's own coast is
    /// corrected.
    #[test]
    fn heightmap_cache_reuses_cells_and_corrects_coasts() {
        let terrains = Arc::new(TerrainRegistry::new());
        let levels = Levels::new(256, 63);
        let cache = HeightmapCache::with_capacity(levels, Arc::clone(&terrains), 16);
        let calls = AtomicUsize::new(0);
        let first = cache.get(12, -34, |cell, _, _| {
            calls.fetch_add(1, Ordering::Relaxed);
            cell.terrain = terrains.get_by_id(3);
            cell.value = levels.water(3);
        });
        let second = cache.get(12, -34, |_, _, _| unreachable!());
        assert_eq!(calls.load(Ordering::Relaxed), 1);
        assert!(first.terrain.is_same(&terrains.get_by_id(4)));
        assert!(second.terrain.is_same(&terrains.get_by_id(4)));
        assert_eq!(first.value.to_bits(), second.value.to_bits());

        let coast = cache.get(13, -34, |cell, _, _| {
            cell.terrain = terrains.get_by_id(3);
            cell.value = levels.water(6);
        });
        assert!(coast.terrain.is_same(&terrains.get_by_id(3)));

        // Another registry's coast is not `TerrainType.COAST`.
        let other = TerrainRegistry::new();
        let foreign = cache.get(14, -34, |cell, _, _| {
            cell.terrain = other.get_by_id(3);
            cell.value = levels.water(3);
        });
        assert!(foreign.terrain.is_same(&other.get_by_id(3)));
    }

    /// A full cache drops its oldest cell first, and threads sharing it get equal cells.
    #[test]
    fn heightmap_cache_evicts_oldest_and_is_shared() {
        let terrains = Arc::new(TerrainRegistry::new());
        let cache = HeightmapCache::with_capacity(Levels::new(256, 63), Arc::clone(&terrains), 2);
        let calls = AtomicUsize::new(0);
        let fill = |cell: &mut Cell, x: f32, _: f32| {
            calls.fetch_add(1, Ordering::Relaxed);
            cell.value = x;
        };
        for x in [1, 2, 3, 2, 1] {
            assert_eq!(cache.get(x, 0, fill).value, x as f32);
        }
        // 1, 2, 3 (drops 1), 2 (hit), 1 (drops 2).
        assert_eq!(calls.load(Ordering::Relaxed), 4);

        let cache = HeightmapCache::new(Levels::new(256, 63), Arc::clone(&terrains));
        std::thread::scope(|scope| {
            for _ in 0..4 {
                scope.spawn(|| {
                    for x in -300..300 {
                        let cell = cache.get(x, -x, |cell, x, z| cell.value = x * 0.5 + z);
                        assert_eq!(cell.value.to_bits(), (x as f32 * 0.5 - x as f32).to_bits());
                    }
                });
            }
        });
    }

    /// Engine 0.3.0's `HeightmapCache.get` against `HeightmapCacheOracle.java` (in the maintainer's
    /// `.local/terraforged/oracles/heightmap/`), with `Heightmap.apply` replaced by the same deterministic fill on
    /// both sides: values around the water and beach levels, coast, beach, flats and none terrains, three
    /// height / sea level variants, caches of 4096 and 16 cells, extreme and negative blocks, each looked up twice.
    #[test]
    fn heightmap_cache_matches_engine() {
        let mut points = Vec::new();
        for i in -12..=12 {
            for j in -12..=12 {
                points.push((37 * i, -41 * j + 5));
            }
        }
        let odd = [i32::MIN, i32::MAX, 16_777_217, -16_777_217, 0, -1];
        for a in odd {
            for b in odd {
                points.push((a, b));
            }
        }
        let registry = Arc::new(TerrainRegistry::new());
        let kinds = [3, 3, 4, 8, 0].map(|id| registry.get_by_id(id));
        let mut fold = Fold(17);
        let mut count = 0;
        for (height, sea_level) in [(256, 63), (384, 62), (100, 200)] {
            let levels = Levels::new(height, sea_level);
            let (w, b) = (levels.water, levels.water(5));
            let values = [
                w,
                w.next_up(),
                w.next_down(),
                b,
                b.next_up(),
                b.next_down(),
                levels.water(3),
                levels.water(6),
                NAN,
                0.0,
                -0.0,
                1.0,
            ];
            for size in [HeightmapCache::CACHE_SIZE, 16] {
                let cache = HeightmapCache::with_capacity(levels, Arc::clone(&registry), size);
                let fills = AtomicUsize::new(0);
                for _ in 0..2 {
                    for &(x, z) in &points {
                        let cell = cache.get(x, z, |cell, x, z| {
                            fills.fetch_add(1, Ordering::Relaxed);
                            // `Float.floatToIntBits`: neither coordinate is NaN.
                            let k = (x.to_bits() as i32)
                                .wrapping_mul(31)
                                .wrapping_add(z.to_bits() as i32);
                            cell.terrain = kinds[k.rem_euclid(5) as usize].clone();
                            cell.value =
                                values[k.wrapping_mul(7).wrapping_add(3).rem_euclid(12) as usize];
                            cell.moisture = x;
                            cell.temperature = z;
                            cell.continent_x = k;
                        });
                        fold.cell(&cell, &registry);
                        count += 1;
                    }
                }
                if size == HeightmapCache::CACHE_SIZE {
                    fold.mix(fills.load(Ordering::Relaxed) as i32);
                }
            }
        }
        assert_eq!((count, fold.0), (7_932, 1_547_846_148));
    }
}
