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
        CurveFunction, DistanceFunction, Domain, EdgeFunction, Interpolation, Module, Source,
        SourceConfig, SourceKind,
    },
    terraforged_blend::{Blender, ContinentLerper2, ContinentLerper3},
    terraforged_climate::Climate,
    terraforged_context::GeneratorContext,
    terraforged_continent_api::{Continent, create_continent_with_rivers},
    terraforged_engine::{Cell, Levels, Terrain, TerrainFormSettings, TerrainRegistry},
    terraforged_populator::{Populator, ScaledPopulator, TerrainPopulator, VolcanoPopulator},
    terraforged_provider::StandardTerrainProvider,
    terraforged_region::{RegionConfig, RegionLerper, RegionModule, RegionSelector},
    terraforged_river::Rivermap,
    terraforged_tile_generator::HeightmapSource,
    terraforged_util::pack,
};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

pub struct Heightmap {
    continent: Arc<dyn Continent>,
    region_module: Arc<RegionModule>,
    levels: Levels,
    climate: Climate,
    root: Arc<dyn Populator>,
    terrain_provider: StandardTerrainProvider,
    terrain_frequency: f32,
    terrains: TerrainRegistry,
}

impl Heightmap {
    pub fn new(context: &mut GeneratorContext) -> Self {
        let settings = context.settings;
        let world = settings.world;
        let control_points = super::terraforged_engine::ControlPoints::new(world.control_points);
        let region_seed = context.seed.offset(789_124);
        let mut region_warp = context.seed.offset(8_934);
        let region_config = RegionConfig::new(
            region_seed.get(),
            settings.terrain.general.terrain_region_size,
            simplex(region_warp.next(), 400, 1),
            simplex(region_warp.next(), 400, 1),
            200.0,
        );
        let levels = context.levels;
        let terrain_frequency = 1.0 / settings.terrain.general.global_horizontal_scale;
        let region_module = Arc::new(RegionModule::new(region_config.clone()));

        let terrains = TerrainRegistry::new();
        let default_populator: Arc<dyn Populator> = Arc::new(TerrainPopulator::of(
            terrains.get_by_id(0),
            Module::constant(0.0),
        ));
        let mut terrain_provider = StandardTerrainProvider::new(
            context.seed,
            levels,
            settings.terrain,
            region_config,
            default_populator,
        );
        let terrain_regions: Arc<dyn Populator> =
            Arc::new(RegionSelector::new(terrain_provider.populators()));
        let terrain_region_borders = configured_populator(
            terrains.get_by_id(8),
            terrain_provider.landforms().land_base(),
            terrain_provider.landforms().plains(&mut context.seed),
            settings.terrain.steppe,
        );
        let terrain: Arc<dyn Populator> =
            Arc::new(RegionLerper::new(terrain_region_borders, terrain_regions));

        let mut mountain_seed = context
            .seed
            .offset(settings.terrain.general.terrain_seed_offset);
        let mountain_shape = Module::cell_edge_source(
            SourceConfig {
                seed: mountain_seed.next(),
                frequency: 1.0 / 1_000.0,
                ..SourceConfig::default()
            },
            1.0,
            EdgeFunction::Distance2Add,
            DistanceFunction::Euclidean,
        )
        .warp(noise_warp(mountain_seed.next(), 333, 2, 250.0))
        .curve(CurveFunction::Interpolation(Interpolation::Curve3))
        .clamp(Module::constant(0.0), Module::constant(0.9))
        .map(Module::constant(0.0), Module::constant(1.0));
        let mountains = configured_populator(
            terrains.get_by_id(13),
            terrain_provider.landforms().land_base(),
            terrain_provider.landforms().mountains(&mut mountain_seed),
            settings.terrain.mountains,
        );
        terrain_provider.register_mixable(Arc::clone(&mountains));

        let continent = create_continent_with_rivers(
            world.continent.continent_type,
            &mut context.seed,
            world,
            settings.rivers,
            levels,
        );
        let climate_continent = Arc::clone(&continent);
        let climate = Climate::new(
            Arc::new(move |x, z| climate_continent.land_value(x, z)),
            &mut context.seed,
            settings.climate,
            world.control_points,
            levels,
        );

        let land: Arc<dyn Populator> = Arc::new(Blender::with_module(
            mountain_shape,
            terrain,
            mountains,
            0.3,
            0.8,
            0.575,
        ));
        let deep_ocean_variance = terrain_provider.landforms().deep_ocean(context.seed.next());
        let deep_ocean = register(
            &mut terrain_provider,
            TerrainPopulator::of(terrains.get_by_id(1), deep_ocean_variance),
        );
        let shallow_ocean = register(
            &mut terrain_provider,
            TerrainPopulator::of(terrains.get_by_id(2), Module::constant(levels.water(-7))),
        );
        let coast = register(
            &mut terrain_provider,
            TerrainPopulator::of(terrains.get_by_id(3), Module::constant(levels.water)),
        );
        let oceans: Arc<dyn Populator> = Arc::new(ContinentLerper3::new(
            deep_ocean,
            shallow_ocean,
            coast,
            control_points.deep_ocean,
            control_points.shallow_ocean,
            control_points.coast,
        ));
        let root: Arc<dyn Populator> = Arc::new(ContinentLerper2::new(
            oceans,
            land,
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
            terrains,
        }
    }

    pub fn apply(&self, cell: &mut Cell, x: f32, z: f32) {
        self.apply_base(cell, x, z);
        let rivermap = self.rivermap(cell.continent_x, cell.continent_z);
        self.apply_rivers(cell, x, z, &rivermap);
        self.apply_climate(cell, x, z);
    }

    pub fn apply_base(&self, cell: &mut Cell, x: f32, z: f32) {
        cell.terrain = self.terrains.get_by_id(8);
        self.continent.apply(cell, x, z);
        self.region_module.apply(cell, x, z);
        self.root
            .apply(cell, x * self.terrain_frequency, z * self.terrain_frequency);
    }

    pub fn apply_rivers(&self, cell: &mut Cell, x: f32, z: f32, rivermap: &Rivermap) {
        rivermap.apply(cell, x, z);
        VolcanoPopulator::modify_volcano_type(cell, self.levels, &self.terrains);
    }

    pub fn apply_climate(&self, cell: &mut Cell, x: f32, z: f32) {
        self.climate.apply(cell, x, z);
    }

    pub fn rivermap(&self, x: i32, z: i32) -> Arc<Rivermap> {
        self.continent.rivermap(x, z)
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

fn configured_populator(
    terrain: Terrain,
    base: Module,
    variance: Module,
    settings: TerrainFormSettings,
) -> Arc<dyn Populator> {
    if settings.vertical_scale == 1.0 && settings.base_scale == 1.0 {
        Arc::new(TerrainPopulator::new(
            terrain,
            base,
            variance,
            settings.weight,
        ))
    } else {
        Arc::new(ScaledPopulator::new(
            terrain,
            base,
            variance,
            settings.base_scale,
            settings.vertical_scale,
            settings.weight,
        ))
    }
}

fn register(
    provider: &mut StandardTerrainProvider,
    populator: TerrainPopulator,
) -> Arc<dyn Populator> {
    let populator: Arc<dyn Populator> = Arc::new(populator);
    provider.register_mixable(Arc::clone(&populator));
    populator
}

fn simplex(seed: i32, scale: i32, octaves: usize) -> Module {
    source(SourceKind::Simplex, seed, scale, octaves)
}

fn source(kind: SourceKind, seed: i32, scale: i32, octaves: usize) -> Module {
    Module::source(Source::new(
        kind,
        SourceConfig {
            seed,
            octaves,
            frequency: 1.0 / scale as f32,
            ..SourceConfig::default()
        },
    ))
}

fn noise_warp(seed: i32, scale: i32, octaves: usize, strength: f32) -> Domain {
    Domain::domain_warp(
        source(SourceKind::Perlin, seed, scale, octaves),
        source(SourceKind::Perlin, seed.wrapping_add(1), scale, octaves),
        Module::constant(strength),
    )
}

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

    #[test]
    fn heightmap_base_matches_engine_0_3_0() {
        let mut settings = super::super::terraforged_context::GeneratorSettings::default();
        settings.world.seed = 1_234;
        let map = Heightmap::new(&mut GeneratorContext::new(settings));
        let cases = [
            (
                0.0,
                0.0,
                [0x3ec8142c, 0x3ef611d2, 0x3f08e3b6, 0x3e8448e4, 0x3ed9c8bf],
                -5_109,
                2_905,
                "flats-hills",
            ),
            (
                12.25,
                -98.75,
                [0x3efce5e4, 0x3ef611d2, 0x3f219fd8, 0x3e8448e4, 0x3f800000],
                -5_109,
                2_905,
                "flats-hills",
            ),
            (
                -4096.5,
                8192.25,
                [0x3e12539a, 0x3ef611d2, 0x00000000, 0x3f6f44c9, 0x3f0bfd8b],
                -5_109,
                2_905,
                "deep_ocean",
            ),
            (
                100_000.0,
                -100_000.0,
                [0x3eebd0e1, 0x3f400000, 0x3f1f5db9, 0x3f5b3d2a, 0x3f3466bf],
                97_853,
                -103_212,
                "mountain_chain",
            ),
        ];
        for (x, z, expected, continent_x, continent_z, terrain) in cases {
            let mut cell = Cell::default();
            map.apply_base(&mut cell, x, z);
            assert_eq!(
                [
                    cell.value.to_bits(),
                    cell.continent_id.to_bits(),
                    cell.continent_edge.to_bits(),
                    cell.terrain_region_id.to_bits(),
                    cell.terrain_region_edge.to_bits(),
                ],
                expected
            );
            assert_eq!(
                (cell.continent_x, cell.continent_z),
                (continent_x, continent_z)
            );
            assert_eq!(cell.terrain.name(), terrain);
        }
    }

    #[test]
    fn heightmap_matches_engine_0_3_0() {
        use super::super::terraforged_engine::BiomeType;

        let mut settings = super::super::terraforged_context::GeneratorSettings::default();
        settings.world.seed = 1_234;
        let map = Heightmap::new(&mut GeneratorContext::new(settings));
        let cases = [
            (
                0.0,
                0.0,
                [
                    0x3ec8142c, 0x3f800000, 0x3f0a4c5a, 0x3f22637f, 0x3e93f3fc, 0x3ebca8f4,
                    0x3f10082e,
                ],
                "coast",
                BiomeType::TemperateForest,
            ),
            (
                12.25,
                -98.75,
                [
                    0x3efce5e4, 0x3f800000, 0x3f03b261, 0x3f164762, 0x3d50a340, 0x3f492d16,
                    0x3f10082e,
                ],
                "flats-hills",
                BiomeType::TemperateForest,
            ),
            (
                -4096.5,
                8192.25,
                [
                    0x3e12539a, 0x3f800000, 0x3f10f5a6, 0x3f15a698, 0x3e91b55b, 0x3e47df98,
                    0x3f37652a,
                ],
                "deep_ocean",
                BiomeType::TemperateForest,
            ),
            (
                100_000.0,
                -100_000.0,
                [
                    0x3eebd0e1, 0x3f800000, 0x3f6460a4, 0x3f7aa36f, 0x3f128ff0, 0x3f3714ab,
                    0x3f056f69,
                ],
                "mountain_chain",
                BiomeType::TropicalRainforest,
            ),
        ];
        for (x, z, expected, terrain, biome) in cases {
            let mut cell = Cell::default();
            map.apply(&mut cell, x, z);
            assert_eq!(
                [
                    cell.value.to_bits(),
                    cell.river_mask.to_bits(),
                    cell.temperature.to_bits(),
                    cell.moisture.to_bits(),
                    cell.biome_region_id.to_bits(),
                    cell.biome_region_edge.to_bits(),
                    cell.macro_biome_id.to_bits(),
                ],
                expected
            );
            assert_eq!(cell.terrain.name(), terrain);
            assert_eq!(cell.biome, biome);
        }
    }
}
