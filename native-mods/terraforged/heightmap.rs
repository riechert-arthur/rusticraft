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
    terraforged::{
        CurveFunction, DistanceFunction, EdgeFunction, Interpolation, Module, SourceConfig,
        SourceKind,
    },
    terraforged_blend::{Blender, ContinentLerper2, ContinentLerper3},
    terraforged_climate::Climate,
    terraforged_context::GeneratorContext,
    terraforged_continent_api::Continent,
    terraforged_engine::{
        Cell, ControlPoints, Levels, Terrain, TerrainFormSettings, TerrainRegistry,
    },
    terraforged_landforms::{noise_warp, source},
    terraforged_populator::{Populator, TerrainPopulator, TerrainPopulatorLike, VolcanoPopulator},
    terraforged_provider::StandardTerrainProvider,
    terraforged_region::{RegionConfig, RegionLerper, RegionModule, RegionSelector},
    terraforged_river::Rivermap,
    terraforged_tile_generator::HeightmapSource,
    terraforged_util::pack,
};
use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
};

/// Engine 0.3.0's `Heightmap`: the world's continent, terrain regions, terrain graph (land, mountains and oceans
/// blended by the continent value), river stage and climate. [`Self::apply`] (Engine's `apply`) runs
/// [`Self::apply_base`], the river stage on the river map of the cell's continent cell, and [`Self::apply_climate`].
/// The stages also plug into [`HeightmapCache`] and the tile fills as closures (see [`HeightmapSource`]).
pub struct Heightmap {
    continent: Arc<dyn Continent>,
    region_module: RegionModule,
    levels: Levels,
    climate: Climate,
    root: Arc<dyn Populator>,
    terrain_provider: StandardTerrainProvider,
    terrain_frequency: f32,
    rivers: HeightmapRivers,
    flats: Terrain,
}

impl Heightmap {
    pub const MOUNTAIN_SCALE: i32 = 1_000;
    pub const DEEP_OCEAN_VALUE: f32 = 0.1;
    pub const SHALLOW_OCEAN_VALUE: f32 = 0.25;
    pub const BEACH_VALUE: f32 = 0.327;
    pub const COAST_VALUE: f32 = 0.448;
    pub const INLAND_VALUE: f32 = 0.502;
    const REGION_SEED_OFFSET: i32 = 789_124;
    const WARP_SEED_OFFSET: i32 = 8_934;

    /// `new Heightmap(context)`, in Engine's order: the region config (from the seed's root, no draws), the
    /// mountain shape (two draws from the mountain seed), the terrain provider (`StandardTerrainProvider`, its own
    /// seed) and its shuffled populators, the region borders (`plains` drawing from `context.seed`), the mountain
    /// chain, the continent (`context.continent()`), the climate (`context.climate`) and the deep ocean (one more
    /// draw from `context.seed`).
    ///
    /// The provider's composite terrains go into the context's registry, as Engine's go into its static
    /// `TerrainType`, so the cells' terrains compare by identity with the registry the continent, climate and
    /// river stage hold.
    ///
    /// # Panics
    ///
    /// If anything else already holds `context.terrain` (a `copy`/`split` of the context, a continent, a climate,
    /// a cache...): registering needs the registry before it is shared.
    pub fn new(context: &mut GeneratorContext) -> Self {
        let settings = context.settings;
        let world = settings.world;
        let general = settings.terrain.general;
        let control_points = ControlPoints::new(world.control_points);
        let region_seed = context.seed.offset(Self::REGION_SEED_OFFSET);
        let mut region_warp = context.seed.offset(Self::WARP_SEED_OFFSET);
        let region_config = RegionConfig::new(
            region_seed.get(),
            general.terrain_region_size,
            simplex(region_warp.next()),
            simplex(region_warp.next()),
            200.0,
        );
        let levels = context.levels;
        let terrain_frequency = 1.0 / general.global_horizontal_scale;
        let region_module = RegionModule::new(region_config.clone());
        let mut mountain_seed = context.seed.offset(general.terrain_seed_offset);
        // `Source.cellEdge(seed, 1000, DISTANCE_2_ADD).warp(seed, 333, 2, 250.0)`.
        let mountain_shape = Module::cell_edge_source(
            SourceConfig {
                seed: mountain_seed.next(),
                octaves: 1,
                gain: 0.5,
                frequency: 1.0 / Self::MOUNTAIN_SCALE as f32,
                lacunarity: 2.0,
                interpolation: Interpolation::Curve3,
            },
            1.0,
            EdgeFunction::Distance2Add,
            DistanceFunction::Euclidean,
        )
        .warp(noise_warp(mountain_seed.next(), 333, 2, 250.0))
        .curve(CurveFunction::Interpolation(Interpolation::Curve3))
        .clamp(Module::constant(0.0), Module::constant(0.9))
        .map(Module::constant(0.0), Module::constant(1.0));

        let terrains = &context.terrain;
        let (flats, deep_ocean, shallow_ocean, coast, mountain_chain) = (
            terrains.get_by_id(8),
            terrains.get_by_id(1),
            terrains.get_by_id(2),
            terrains.get_by_id(3),
            terrains.get_by_id(13),
        );
        // Engine passes the heightmap itself as the default populator; `Self::populator` returns `self` there.
        let placeholder: Arc<dyn Populator> = Arc::new(TerrainPopulator::of(
            terrains.get_by_id(0),
            Module::constant(0.0),
        ));
        let mut terrain_provider = StandardTerrainProvider::new(
            context.seed,
            levels,
            settings.terrain,
            region_config,
            placeholder,
            terrains,
        );
        let registry = Arc::get_mut(&mut context.terrain).expect(
            "Heightmap::new registers composite terrains: the context's registry must not be shared yet",
        );
        let populators = terrain_provider.populators(registry);
        let terrain_regions = RegionSelector::new(
            populators
                .into_iter()
                .map(|populator| -> Arc<dyn Populator> { populator })
                .collect(),
        );
        let land_base = terrain_provider.landforms().land_base();
        let terrain_region_borders = form(
            flats.clone(),
            land_base.clone(),
            terrain_provider.landforms().plains(&mut context.seed),
            settings.terrain.steppe,
        );
        let terrain = RegionLerper::new(terrain_region_borders, Arc::new(terrain_regions));
        let mountains = form(
            mountain_chain,
            land_base,
            terrain_provider.landforms().mountains(&mut mountain_seed),
            settings.terrain.mountains,
        );
        terrain_provider.register_mixable(Arc::clone(&mountains));
        let continent = context.continent();
        let climate = context.climate(continent.clone());
        let land = Blender::with_module(
            mountain_shape,
            Arc::new(terrain),
            mountains,
            0.3,
            0.8,
            0.575,
        );
        // Java evaluates `ContinentLerper3`'s arguments left to right: the deep ocean draws, then the three
        // populators are registered in order.
        let deep_ocean_variance = terrain_provider.landforms().deep_ocean(context.seed.next());
        let mut register = |terrain, variance| -> Arc<dyn Populator> {
            let populator: Arc<dyn TerrainPopulatorLike> =
                Arc::new(TerrainPopulator::of(terrain, variance));
            terrain_provider.register_mixable(Arc::clone(&populator));
            populator
        };
        let oceans = ContinentLerper3::new(
            register(deep_ocean, deep_ocean_variance),
            register(shallow_ocean, Module::constant(levels.water(-7))),
            register(coast, Module::constant(levels.water)),
            control_points.deep_ocean,
            control_points.shallow_ocean,
            control_points.coast,
        );
        let root = Arc::new(ContinentLerper2::new(
            Arc::new(oceans),
            Arc::new(land),
            control_points.shallow_ocean,
            control_points.inland,
        ));
        Self {
            continent,
            region_module,
            levels,
            climate,
            root,
            terrain_provider,
            terrain_frequency,
            rivers: HeightmapRivers::new(levels, Arc::clone(&context.terrain)),
            flats,
        }
    }

    /// `apply(cell, x, z)`: [`Self::apply_base`], the river stage on the river map of the cell's continent cell
    /// (Engine's private `applyRivers(cell, x, z)`, [`HeightmapRivers::apply_continent`]), then
    /// [`Self::apply_climate`], all at the unscaled block position.
    pub fn apply(&self, cell: &mut Cell, x: f32, z: f32) {
        self.apply_base(cell, x, z);
        self.rivers
            .apply_continent(cell, x, z, |cx, cz| self.continent.rivermap(cx, cz));
        self.apply_climate(cell, x, z);
    }

    /// `applyBase(cell, x, z)`: the terrain starts as `flats`; the continent and the terrain regions sample the
    /// block position, the terrain graph (and so the volcanoes among the region populators) samples it times
    /// `terrainFrequency` (`1 / globalHorizontalScale`), in f32 as Engine's `x *= terrainFrequency`.
    pub fn apply_base(&self, cell: &mut Cell, x: f32, z: f32) {
        cell.terrain = self.flats.clone();
        self.continent.apply(cell, x, z);
        self.region_module.apply(cell, x, z);
        self.root
            .apply(cell, x * self.terrain_frequency, z * self.terrain_frequency);
    }

    /// `applyRivers(cell, x, z, rivermap)`: [`HeightmapRivers::apply`].
    pub fn apply_rivers(&self, cell: &mut Cell, x: f32, z: f32, rivermap: &Rivermap) {
        self.rivers.apply(cell, x, z, rivermap);
    }

    /// `applyClimate(cell, x, z)`: [`Climate::apply`].
    pub fn apply_climate(&self, cell: &mut Cell, x: f32, z: f32) {
        self.climate.apply(cell, x, z);
    }

    /// `getRivermap(x, z)`: the continent's river map of a continent cell.
    pub fn rivermap(&self, x: i32, z: i32) -> Arc<Rivermap> {
        self.continent.rivermap(x, z)
    }

    /// `getPopulator(terrain, id)`: the provider's variant, or, for a negative variant or an unregistered terrain,
    /// the heightmap itself (Engine's default populator). [`Self::terrain_provider`]'s own `populator` returns a
    /// zero placeholder there instead.
    pub fn populator(&self, terrain: &Terrain, variant: i32) -> &dyn Populator {
        if variant < 0 || self.terrain_provider.variant_count(terrain) == 0 {
            self
        } else {
            self.terrain_provider.populator(terrain, variant)
        }
    }

    pub fn continent(&self) -> &Arc<dyn Continent> {
        &self.continent
    }

    pub fn region_module(&self) -> &RegionModule {
        &self.region_module
    }

    pub const fn levels(&self) -> Levels {
        self.levels
    }

    pub const fn climate(&self) -> &Climate {
        &self.climate
    }

    pub fn terrain_provider(&self) -> &StandardTerrainProvider {
        &self.terrain_provider
    }
}

impl Populator for Heightmap {
    fn apply(&self, cell: &mut Cell, x: f32, z: f32) {
        Heightmap::apply(self, cell, x, z);
    }
}

impl HeightmapSource for Heightmap {
    fn apply_base(&self, cell: &mut Cell, x: f32, z: f32) {
        Heightmap::apply_base(self, cell, x, z);
    }

    fn rivermap(&self, x: i32, z: i32) -> Arc<Rivermap> {
        Heightmap::rivermap(self, x, z)
    }

    fn apply_rivers(&self, cell: &mut Cell, x: f32, z: f32, rivermap: &Rivermap) {
        Heightmap::apply_rivers(self, cell, x, z, rivermap);
    }

    fn apply_climate(&self, cell: &mut Cell, x: f32, z: f32) {
        Heightmap::apply_climate(self, cell, x, z);
    }
}

/// `TerrainPopulator.of(terrain, base, variance, settings)`.
fn form(
    terrain: Terrain,
    base: Module,
    variance: Module,
    settings: TerrainFormSettings,
) -> Arc<dyn TerrainPopulatorLike> {
    Arc::from(TerrainPopulator::of_with_settings(
        terrain, base, variance, settings,
    ))
}

/// `Source.simplex(seed, 400, 1)`, the region warp axes.
fn simplex(seed: i32) -> Module {
    source(SourceKind::Simplex, seed, 400, 1, 0.5, 2.0)
}

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

/// Engine 0.3.0's `HeightmapCache`: memoizes the heightmap's cell of each block (keyed by `PosUtil.pack(x, z)`).
/// [`Self::get`] (`get(x, z)`) turns, on a miss, a coast cell whose value is above the water level and at most five
/// blocks over it into a beach (the same correction as `WorldLookup.compute`); [`Self::generate`] (the tile path)
/// fills the caller's cell and does not. Both share one cache.
///
/// Both take the heightmap's stages as closures (e.g. [`Heightmap::apply`]'s). Engine's `get` returns the shared
/// cached `Cell` object; this returns a copy (every field, as `Clone`), so callers cannot change what the cache
/// holds.
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
        self.insert(id, &cell);
        cell
    }

    /// `generate(cell, x, z, rivermap)`, the tile path (`Tile.generate(HeightmapCache, ...)`): on a hit, copies the
    /// cached cell into `cell` (`copyFrom`, which keeps `cell`'s `terrain_region_center`) and returns `current`
    /// unchanged; on a miss, runs Engine's `contextCompute` on `cell` itself (not reset first): `apply_base`
    /// (`heightmap.applyBase`), the river map of the cell's continent cell, reusing `current` when it matches
    /// (`Rivermap.get(cell, rivermap, heightmap)`, with `lookup` as `heightmap.getRivermap`), `apply_rivers`
    /// (`heightmap.applyRivers(cell, x, z, rivermap)`, e.g. [`HeightmapRivers::apply`]) and `apply_climate`
    /// (`heightmap.applyClimate`), all with the block coordinates as floats, then caches a copy of `cell` and
    /// returns the map it used. No beach correction is made on this path, as in Engine.
    ///
    /// Engine caches the caller's `Cell` object itself, so whatever the tile later does to that cell also changes
    /// the cached entry (and a later `get` or `generate` of the block sees it); this caches a snapshot taken right
    /// after `apply_climate`. As with [`Self::get`], misses run outside the lock: two threads missing the same
    /// block both compute it (equal cells), and each returns the map it used where Engine's second thread would hit
    /// and return its own `current`.
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
        if let Some(cached) = self.lock().map.get(&id) {
            cell.copy_from(cached);
            return current;
        }
        let (sample_x, sample_z) = (x as f32, z as f32);
        apply_base(cell, sample_x, sample_z);
        let rivermap = Rivermap::get_for_cell(cell, current, lookup);
        apply_rivers(cell, sample_x, sample_z, &rivermap);
        apply_climate(cell, sample_x, sample_z);
        self.insert(id, cell);
        Some(rivermap)
    }

    /// Caches `cell` for `id` unless another thread already did, dropping the oldest cells while full.
    fn insert(&self, id: i64, cell: &Cell) {
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
    }

    /// The cells. The heightmap stages run outside the lock, so a poisoned lock (a panic while inserting) still
    /// guards a consistent map.
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

    /// A miss runs the base, river and climate stages in that order and reuses the caller's matching map; a hit
    /// runs no stage and returns the caller's map.
    #[test]
    fn contextual_heightmap_cache_runs_stages_once() {
        let terrains = Arc::new(TerrainRegistry::new());
        let cache = HeightmapCache::with_capacity(Levels::new(256, 63), Arc::clone(&terrains), 16);
        let map = Arc::new(Rivermap::new(17, -9, Vec::new(), GenWarp::direct()));
        let order = Mutex::new(Vec::new());
        let mut cell = Cell::new(&terrains);
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

        let mut cached = Cell::new(&terrains);
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

    /// Engine 0.3.0's `HeightmapCache.generate` mixed with `get` against `HeightmapGenerateOracle.java` (in the
    /// maintainer's `.local/terraforged/oracles/heightmap/`), with the heightmap's stages replaced by the same fills
    /// on both sides and a continent whose `getRivermap` makes a new, counted map: 600 calls over 120 blocks, caller
    /// cells pre-filled with markers that a miss keeps and a hit's `copyFrom` overwrites (or, for the terrain
    /// region center, keeps), the current map dropped every 50 calls.
    #[test]
    fn contextual_heightmap_cache_matches_engine() {
        use std::cell::RefCell;
        let registry = Arc::new(TerrainRegistry::new());
        let terrains = [3, 4, 8, 0].map(|id| registry.get_by_id(id));
        let fold = RefCell::new(Fold(17));
        let mix = |v: i32| fold.borrow_mut().mix(v);
        let lookups = std::cell::Cell::new(0);
        let base = |cell: &mut Cell, x: f32, z: f32| {
            mix(1);
            let k = (x.to_bits() as i32)
                .wrapping_mul(31)
                .wrapping_add(z.to_bits() as i32);
            cell.terrain = terrains[k.rem_euclid(4) as usize].clone();
            cell.value = x * 0.25 + z;
            cell.moisture += x;
            cell.continent_x = k.rem_euclid(3);
            cell.continent_z = (k >> 2).rem_euclid(2);
        };
        let rivers = |cell: &mut Cell, _: f32, _: f32, map: &Rivermap| {
            mix(2);
            cell.river_mask = (map.x() * 10 + map.z()) as f32 + cell.value;
        };
        let climate = |cell: &mut Cell, _: f32, z: f32| {
            mix(3);
            cell.temperature = z - cell.river_mask;
        };
        let lookup = |x: i32, z: i32| {
            lookups.set(lookups.get() + 1);
            mix(1000 + lookups.get());
            Arc::new(Rivermap::new(x, z, Vec::new(), GenWarp::direct()))
        };
        let cache =
            HeightmapCache::with_capacity(Levels::new(256, 63), Arc::clone(&registry), 4096);
        let mut current: Option<Arc<Rivermap>> = None;
        for i in 0..600_i32 {
            let p = (i * 37 + (i / 120) * 11).rem_euclid(120);
            let (x, z) = ((p % 12) * 13 - 70, (p / 12) * -29 + 100);
            if i % 7 == 3 {
                let cell = cache.get(x, z, |cell, x, z| {
                    mix(4);
                    base(cell, x, z);
                    let map = Rivermap::new(
                        cell.continent_x,
                        cell.continent_z,
                        Vec::new(),
                        GenWarp::direct(),
                    );
                    rivers(cell, x, z, &map);
                    climate(cell, x, z);
                });
                fold.borrow_mut().cell(&cell, &registry);
            } else {
                let mut cell = Cell::new(&registry);
                cell.erosion = i as f32 * 0.5;
                cell.moisture = i as f32;
                cell.terrain_region_center = 0x1234_5678_9abc_i64.wrapping_mul(i64::from(i));
                let before = current.clone();
                current = cache.generate(&mut cell, x, z, current, base, lookup, rivers, climate);
                mix(current.as_ref().map_or(-1, |map| map.x()));
                mix(current.as_ref().map_or(-1, |map| map.z()));
                // Java's `current == before`: the same map, or both null.
                let same = match (&current, &before) {
                    (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                    (a, b) => a.is_none() && b.is_none(),
                };
                mix(i32::from(same));
                fold.borrow_mut().cell(&cell, &registry);
            }
            if i % 50 == 49 {
                current = None;
            }
        }
        assert_eq!((lookups.get(), fold.into_inner().0), (72, -383_835_515));
    }

    /// Engine 0.3.0's `Heightmap` against `HeightmapGraphOracle.java` (in the maintainer's
    /// `.local/terraforged/oracles/heightmap_graph/`), which builds the real `new Heightmap(GeneratorContext
    /// .createNoCache(settings))` for four settings variants (seeds, `globalHorizontalScale` 1, 2.5, 0.37 and 4, region
    /// sizes, seed offsets, scaled and zero-weight landforms, levels, control points) and every continent type: the
    /// context seed's cursor before and after the constructor, the provider's variant count of each built-in terrain,
    /// and every cell field after `apply` over 196 edge points (signed zeros, a subnormal, huge values, ±infinity,
    /// NaN; some printed in full) and a 40x40 grid, with terrains by name and checked by identity against the
    /// context's registry. The grids hold volcano cells, which sample the terrain graph's scaled coordinates.
    #[test]
    fn heightmap_matches_engine_0_3_0() {
        const EXPECTED: &str = "\
BUILD 0 MULTI 1234 1251
VARIANTS 0 MULTI 0 1 1 1 0 0 0 0 2 2 1 4 3 1 1 0
EDGES 0 MULTI fold eba43bf9
GRID 0 MULTI fold 23f9fbd4 volcanoes 79 throws 0
BUILD 0 SINGLE 1234 1251
VARIANTS 0 SINGLE 0 1 1 1 0 0 0 0 2 2 1 4 3 1 1 0
EDGES 0 SINGLE fold 309096b9
GRID 0 SINGLE fold 0f71f39e volcanoes 44 throws 0
BUILD 0 MULTI_IMPROVED 1234 1254
VARIANTS 0 MULTI_IMPROVED 0 1 1 1 0 0 0 0 2 2 1 4 3 1 1 0
EDGES 0 MULTI_IMPROVED fold 2eea2e83
POINT 0 MULTI_IMPROVED 00000000 00000000 3ec8142c 00000000 00000000 00000000 3f22637f 3f0a4c5a 3ef611d2 3f08e3b6 3e8448e4 3ed9c8bf 674309864963 3e93f3fc 3ebca8f4 3f10082e 3f800000 -5109 2905 false coast 4
POINT 0 MULTI_IMPROVED 41440000 c2c58000 3efce5e4 00000000 00000000 00000000 3f164762 3f03b261 3ef611d2 3f219fd8 3e8448e4 3f800000 674309864963 3d50a340 3f492d16 3f10082e 3f800000 -5109 2905 false flats-hills 4
POINT 0 MULTI_IMPROVED 4e6e6b28 cbe4e1c0 3de19f0a 00000000 00000000 00000000 00000000 3f800000 3e8dd384 3dd973aa 3e56c35e 3f5fa50e 4294966475631246568 3e0cf8c0 3f51c28f 3ec9e9ce 3f800000 1000000576 -29997950 false ocean 2
POINT 0 MULTI_IMPROVED 7f800000 ff800000 7fc00000 00000000 00000000 00000000 7fc00000 3f066666 3f6a6b14 3f800000 3f40e23c 00000000 0 3e80ea4a 00000000 3edc9c2f 3f800000 0 0 false coast 2
POINT 0 MULTI_IMPROVED 7fc00000 7fc00000 7fc00000 00000000 00000000 00000000 7fc00000 3f066666 3f6a6b14 3f800000 3f40e23c 00000000 0 3e80ea4a 00000000 3edc9c2f 3f800000 0 0 false mountain_chain 2
GRID 0 MULTI_IMPROVED fold 05738642 volcanoes 79 throws 0
BUILD 0 FANCY 1234 1251
VARIANTS 0 FANCY 0 1 1 1 0 0 0 0 2 2 1 4 3 1 1 0
EDGES 0 FANCY fold bb4276a8
GRID 0 FANCY fold 294da34e volcanoes 16 throws 0
BUILD 1 MULTI -987654321 -987654304
VARIANTS 1 MULTI 0 1 1 1 0 0 0 0 2 2 1 4 3 1 1 0
EDGES 1 MULTI fold 00d7f2bd
GRID 1 MULTI fold 14309a5c volcanoes 66 throws 0
BUILD 1 SINGLE -987654321 -987654304
VARIANTS 1 SINGLE 0 1 1 1 0 0 0 0 2 2 1 4 3 1 1 0
EDGES 1 SINGLE fold 171c24e6
GRID 1 SINGLE fold 52ad32f2 volcanoes 19 throws 0
BUILD 1 MULTI_IMPROVED -987654321 -987654301
VARIANTS 1 MULTI_IMPROVED 0 1 1 1 0 0 0 0 2 2 1 4 3 1 1 0
EDGES 1 MULTI_IMPROVED fold 06df14d4
GRID 1 MULTI_IMPROVED fold 88fce55c volcanoes 33 throws 0
BUILD 1 FANCY -987654321 -987654304
VARIANTS 1 FANCY 0 1 1 1 0 0 0 0 2 2 1 4 3 1 1 0
EDGES 1 FANCY fold 35312b05
GRID 1 FANCY fold d8f1951c volcanoes 13 throws 0
BUILD 2 MULTI 591751049 591751066
VARIANTS 2 MULTI 0 1 1 1 0 0 0 0 2 2 1 4 3 1 1 0
EDGES 2 MULTI fold 86338186
GRID 2 MULTI fold b6c74d4a volcanoes 278 throws 0
BUILD 2 SINGLE 591751049 591751066
VARIANTS 2 SINGLE 0 1 1 1 0 0 0 0 2 2 1 4 3 1 1 0
EDGES 2 SINGLE fold 41985fad
GRID 2 SINGLE fold 8b33a5ee volcanoes 6 throws 0
BUILD 2 MULTI_IMPROVED 591751049 591751069
VARIANTS 2 MULTI_IMPROVED 0 1 1 1 0 0 0 0 2 2 1 4 3 1 1 0
EDGES 2 MULTI_IMPROVED fold bc365c9e
GRID 2 MULTI_IMPROVED fold 48584aa1 volcanoes 80 throws 0
BUILD 2 FANCY 591751049 591751066
VARIANTS 2 FANCY 0 1 1 1 0 0 0 0 2 2 1 4 3 1 1 0
EDGES 2 FANCY fold 398969c3
GRID 2 FANCY fold d49a5631 volcanoes 9 throws 0
BUILD 3 MULTI 7 24
VARIANTS 3 MULTI 0 1 1 1 0 0 0 0 2 2 1 4 3 1 1 0
EDGES 3 MULTI fold 29ac04a8
GRID 3 MULTI fold 3524034f volcanoes 52 throws 0
BUILD 3 SINGLE 7 24
VARIANTS 3 SINGLE 0 1 1 1 0 0 0 0 2 2 1 4 3 1 1 0
EDGES 3 SINGLE fold 8dcf8699
GRID 3 SINGLE fold 32d9dbd4 volcanoes 22 throws 0
BUILD 3 MULTI_IMPROVED 7 27
VARIANTS 3 MULTI_IMPROVED 0 1 1 1 0 0 0 0 2 2 1 4 3 1 1 0
EDGES 3 MULTI_IMPROVED fold 906ea0cc
GRID 3 MULTI_IMPROVED fold 4c8133f1 volcanoes 55 throws 0
BUILD 3 FANCY 7 24
VARIANTS 3 FANCY 0 1 1 1 0 0 0 0 2 2 1 4 3 1 1 0
EDGES 3 FANCY fold 2c0cf66d
GRID 3 FANCY fold cf31de76 volcanoes 15 throws 0
";
        use super::super::terraforged_context::GeneratorSettings;
        use super::super::terraforged_engine::ContinentType;
        fn bits(v: f32) -> String {
            format!("{:08x}", v.to_bits())
        }
        /// `String.hashCode` of an ASCII name.
        fn hash(name: &str) -> i32 {
            name.bytes()
                .fold(0_i32, |h, b| h.wrapping_mul(31).wrapping_add(i32::from(b)))
        }
        fn fold_cell(fold: &mut Fold, c: &Cell) {
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
                fold.f(v);
            }
            fold.mix(c.terrain_region_center as i32);
            fold.mix((c.terrain_region_center as u64 >> 32) as i32);
            fold.mix(c.continent_x);
            fold.mix(c.continent_z);
            fold.mix(i32::from(c.erosion_mask));
            fold.mix(hash(c.terrain.name()));
            fold.mix(c.biome as i32);
        }
        fn line(c: &Cell) -> String {
            let floats = [
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
            ];
            let more = [
                c.biome_region_id,
                c.biome_region_edge,
                c.macro_biome_id,
                c.river_mask,
            ];
            let canonical = |v: &f32| bits(if v.is_nan() { f32::NAN } else { *v });
            format!(
                "{} {} {} {} {} {} {} {}",
                floats.iter().map(canonical).collect::<Vec<_>>().join(" "),
                c.terrain_region_center,
                more.iter().map(canonical).collect::<Vec<_>>().join(" "),
                c.continent_x,
                c.continent_z,
                c.erosion_mask,
                c.terrain.name(),
                c.biome as i32,
            )
        }
        let xs = [
            0.0,
            -0.0,
            f32::from_bits(1),
            12.25,
            -98.75,
            -4096.5,
            8192.25,
            100_000.0,
            -3.0e7,
            1.0e9,
            f32::MAX,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::NAN,
        ];
        let shown = [
            (0.0, 0.0),
            (12.25, -98.75),
            (1.0e9, -3.0e7),
            (f32::INFINITY, f32::NEG_INFINITY),
            (f32::NAN, f32::NAN),
        ];
        let names = ["MULTI", "SINGLE", "MULTI_IMPROVED", "FANCY"];
        let builtins = [
            "none",
            "deep_ocean",
            "ocean",
            "coast",
            "beach",
            "river",
            "lake",
            "wetland",
            "flats",
            "badlands",
            "plateau",
            "hills",
            "mountains",
            "mountain_chain",
            "volcano",
            "volcano_pipe",
        ];
        let mut out = String::new();
        for variant in 0..4 {
            for kind in ContinentType::VALUES {
                let mut s = GeneratorSettings::default();
                s.world.continent.continent_type = kind;
                match variant {
                    0 => s.world.seed = 1_234,
                    1 => {
                        s.world.seed = -987_654_321;
                        s.terrain.general.global_horizontal_scale = 2.5;
                        s.terrain.general.terrain_region_size = 800;
                        s.terrain.general.terrain_seed_offset = 77;
                    }
                    2 => {
                        s.world.seed = 0x1_2345_6789;
                        s.terrain.general.global_horizontal_scale = 0.37;
                        s.terrain.mountains.vertical_scale = 1.3;
                        s.terrain.mountains.base_scale = 0.8;
                        s.terrain.steppe.weight = 0.0;
                        s.terrain.hills.weight = 0.0;
                        s.terrain.volcano.weight = 20.0;
                        s.world.properties.world_height = 384;
                        s.world.properties.sea_level = 62;
                        s.world.control_points.inland = 0.6;
                        s.world.continent.continent_scale = 1_500;
                    }
                    _ => {
                        s.world.seed = 7;
                        s.terrain.plains.base_scale = 1.5;
                        s.terrain.torridonian.weight = 0.0;
                        s.terrain.general.global_horizontal_scale = 4.0;
                    }
                }
                let mut context = GeneratorContext::create_no_cache(s);
                let start = context.seed.get();
                let map = Heightmap::new(&mut context);
                let registry = &context.terrain;
                let tag = format!("{variant} {}", names[kind.index() as usize]);
                out += &format!("BUILD {tag} {start} {}\n", context.seed.get());
                out += &format!("VARIANTS {tag}");
                for name in builtins {
                    let terrain = registry.get_by_name(name).unwrap();
                    out += &format!(" {}", map.terrain_provider().variant_count(&terrain));
                }
                out += "\n";
                // Engine's `==`: the cell's terrain is the context registry's own.
                let check = |c: &Cell| {
                    let own = registry.get_by_name(c.terrain.name()).unwrap();
                    assert!(c.terrain.is_same(&own), "{}", c.terrain.name());
                };
                let mut fold = Fold(17);
                let mut points = Vec::new();
                for x in xs {
                    for z in xs {
                        let mut c = Cell::new(registry);
                        map.apply(&mut c, x, z);
                        check(&c);
                        fold_cell(&mut fold, &c);
                        if variant == 0
                            && kind.index() == 2
                            && shown.iter().any(|&(a, b)| {
                                a.to_bits() == x.to_bits() && b.to_bits() == z.to_bits()
                            })
                        {
                            points.push(format!(
                                "POINT {tag} {} {} {}\n",
                                bits(x),
                                bits(z),
                                line(&c)
                            ));
                        }
                    }
                }
                out += &format!("EDGES {tag} fold {:08x}\n", fold.0);
                out += &points.concat();
                let (mut fold, mut volcanoes) = (Fold(17), 0);
                let (volcano, pipe) = (registry.get_by_id(14), registry.get_by_id(15));
                for i in 0..40 {
                    for j in 0..40 {
                        let (x, z) = (-9000.0 + 461.5 * i as f32, -8000.0 + 433.25 * j as f32);
                        let mut c = Cell::new(registry);
                        map.apply(&mut c, x, z);
                        check(&c);
                        fold_cell(&mut fold, &c);
                        if c.terrain.is_same(&volcano) || c.terrain.is_same(&pipe) {
                            volcanoes += 1;
                        }
                    }
                }
                out += &format!(
                    "GRID {tag} fold {:08x} volcanoes {volcanoes} throws 0\n",
                    fold.0
                );
            }
        }
        assert_eq!(out, EXPECTED);
    }

    /// The heightmap's stages plug into the tile fills and [`HeightmapCache`] unchanged: a tile filled directly or
    /// through the cache (river maps reused across cells by `Rivermap.get`) holds the same cells as `apply` at each
    /// block, and the cache's `get` is `apply` plus its coast-to-beach correction.
    #[test]
    fn heightmap_stages_fill_tiles_and_caches_like_apply() {
        use super::super::terraforged_context::GeneratorSettings;
        use super::super::terraforged_tile::Tile;
        let mut settings = GeneratorSettings::default();
        settings.world.seed = 99;
        let mut context = GeneratorContext::create_no_cache(settings);
        let map = Heightmap::new(&mut context);
        let registry = Arc::clone(&context.terrain);
        let fold = |c: &Cell| {
            let mut fold = Fold(17);
            fold.cell(c, &registry);
            fold.0
        };
        let cache = HeightmapCache::new(map.levels(), Arc::clone(&registry));
        let (mut direct, mut cached) = (
            Tile::new(-1, 2, 1, 0, &registry),
            Tile::new(-1, 2, 1, 0, &registry),
        );
        direct.generate_heightmap(
            |c, x, z| map.apply_base(c, x, z),
            |x, z| map.rivermap(x, z),
            |c, x, z, r| map.apply_rivers(c, x, z, r),
            |c, x, z| map.apply_climate(c, x, z),
        );
        cached.generate_cached_heightmap(
            &cache,
            |c, x, z| map.apply_base(c, x, z),
            |x, z| map.rivermap(x, z),
            |c, x, z, r| map.apply_rivers(c, x, z, r),
            |c, x, z| map.apply_climate(c, x, z),
        );
        let (coast, beach) = (registry.get_by_id(3), registry.get_by_id(4));
        for bx in -32..0 {
            for bz in 64..96 {
                let mut expected = Cell::new(&registry);
                map.apply(&mut expected, bx as f32, bz as f32);
                assert_eq!(fold(direct.cell(bx, bz)), fold(&expected));
                assert_eq!(fold(cached.cell(bx, bz)), fold(&expected));
                let got = cache.get(bx, bz, |_, _, _| unreachable!("tile fill cached it"));
                assert_eq!(fold(&got), fold(&expected));
                let fresh = HeightmapCache::new(map.levels(), Arc::clone(&registry));
                let got = fresh.get(bx, bz, |c, x, z| map.apply(c, x, z));
                if expected.terrain.is_same(&coast)
                    && expected.value > map.levels().water
                    && expected.value <= map.levels().water(5)
                {
                    expected.terrain = beach.clone();
                }
                assert_eq!(fold(&got), fold(&expected));
            }
        }
    }
}
