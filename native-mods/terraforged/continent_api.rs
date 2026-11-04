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

use std::sync::Arc;

use super::{
    terraforged_advanced_continent::AdvancedContinentGenerator,
    terraforged_climate::LandValue,
    terraforged_continent::SimpleContinentGenerator,
    terraforged_engine::{
        Cell, ContinentType, Levels, RiverSettings, Seed, TerrainRegistry, WorldSettings,
    },
    terraforged_fancy::FancyContinentGenerator,
    terraforged_populator::Populator,
    terraforged_river::Rivermap,
};

/// Engine's `Continent`: a [`Populator`] (`apply` writes the continent fields of a cell) with its edge and land
/// values ([`LandValue`], `getLandValue`, which defaults to the edge value in Engine), nearest centre and river maps.
/// An `Arc<dyn Continent>` upcasts to the `Arc<dyn LandValue>` the climate stage takes, as Engine's `Heightmap`
/// passes its continent to `new Climate(continent, context)`.
pub trait Continent: Populator + LandValue {
    /// `getEdgeValue(x, z)`.
    fn edge_value(&self, x: f32, z: f32) -> f32;

    /// `getNearestCenter(x, z)`, packed with `PosUtil.pack`.
    fn nearest_center(&self, x: f32, z: f32) -> i64;

    /// `getRivermap(x, z)`: the river map of continent cell (`x`, `z`), from the continent's river cache.
    fn rivermap(&self, x: i32, z: i32) -> Arc<Rivermap>;

    /// `getRivermap(cell)`: the river map of the cell's continent cell (`continentX`, `continentZ`).
    fn rivermap_for_cell(&self, cell: &Cell) -> Arc<Rivermap> {
        self.rivermap(cell.continent_x, cell.continent_z)
    }
}

macro_rules! continent {
    ($generator:ty) => {
        impl LandValue for $generator {
            fn land_value(&self, x: f32, z: f32) -> f32 {
                <$generator>::land_value(self, x, z)
            }
        }

        impl Continent for $generator {
            fn edge_value(&self, x: f32, z: f32) -> f32 {
                <$generator>::edge_value(self, x, z)
            }

            fn nearest_center(&self, x: f32, z: f32) -> i64 {
                <$generator>::nearest_center(self, x, z)
            }

            fn rivermap(&self, x: i32, z: i32) -> Arc<Rivermap> {
                <$generator>::rivermap(self, x, z)
            }
        }
    };
}

continent!(SimpleContinentGenerator);
continent!(AdvancedContinentGenerator);
continent!(FancyContinentGenerator);

/// `ContinentType.create(seed, context)`: `Multi` builds a `MultiContinentGenerator`, `Single` a
/// `SingleContinentGenerator` ([`SimpleContinentGenerator::multi`]/[`single`](SimpleContinentGenerator::single)),
/// `MultiImproved` an [`AdvancedContinentGenerator`] and `Fancy` a [`FancyContinentGenerator`], each drawing its
/// seeds from `seed` (4, 4, 7 and 4). The rest is what the constructors read from the `GeneratorContext`: `root_seed`
/// is `context.seed.root()`, `settings` and `rivers` are its world and river settings, `levels` is `context.levels`
/// and `registry` is the world's terrain registry. Engine's `Heightmap` calls
/// `settings.continent.continent_type`'s `create` with the context's own seed. Engine's settings deserializer, which
/// reads the type by its exact (case-sensitive) name and keeps the default `MultiImproved` for an unknown one, is
/// not ported.
pub fn create_continent(
    kind: ContinentType,
    seed: &mut Seed,
    root_seed: i32,
    settings: WorldSettings,
    rivers: RiverSettings,
    levels: Levels,
    registry: Arc<TerrainRegistry>,
) -> Arc<dyn Continent> {
    match kind {
        ContinentType::Multi => Arc::new(SimpleContinentGenerator::multi(
            seed, root_seed, settings, rivers, levels, registry,
        )),
        ContinentType::Single => Arc::new(SimpleContinentGenerator::single(
            seed, root_seed, settings, rivers, levels, registry,
        )),
        ContinentType::MultiImproved => Arc::new(AdvancedContinentGenerator::new(
            seed, root_seed, settings, rivers, levels, registry,
        )),
        ContinentType::Fancy => Arc::new(FancyContinentGenerator::new(
            seed, root_seed, settings, rivers, levels, registry,
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::super::terraforged_climate::{Climate, ClimateSettings};
    use super::super::terraforged_engine::{ContinentShape, Lake, River, Wetland, WorldSettings};
    use super::super::terraforged_river::{BaseRiverGenerator, RiverConfig};
    use super::*;

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

    /// `ContinentFactoryOracle.settings(type, variant)`: world and river settings.
    fn settings(kind: ContinentType, variant: i32) -> (WorldSettings, RiverSettings) {
        let mut world = WorldSettings {
            seed: 12_345,
            ..WorldSettings::default()
        };
        world.continent.continent_type = kind;
        let mut rivers = RiverSettings::default();
        let continent = &mut world.continent;
        match variant {
            1 => {
                world.seed = -987_654_321;
                rivers.seed_offset = i32::MAX;
                rivers.lakes.chance = 1.0;
                rivers.wetlands.chance = 1.0;
                rivers.river_count = 3;
                continent.continent_jitter = 0.5;
                continent.continent_skipping = 0.5;
                continent.continent_size_variance = 0.5;
            }
            2 => {
                world.properties.world_height = 384;
                world.properties.sea_level = 62;
                world.control_points.inland = 0.75;
                continent.continent_scale = 1000;
                continent.continent_shape = ContinentShape::Manhattan;
                rivers.river_count = 5;
            }
            3 => {
                world.seed = 7;
                continent.continent_scale = 777;
                continent.continent_shape = ContinentShape::Natural;
                continent.continent_noise_octaves = 2;
                rivers.lakes.chance = 0.5;
            }
            6 => {
                rivers.seed_offset = -123_457;
                rivers.river_count = 14;
                rivers.main_rivers = River::new(7, 3, 9, 31, 11, 0.6);
                // `bankHeight(min, max)` sorts them: the branch's 6 and 2 become 2..6.
                rivers.branch_rivers = River::new(2, 6, 2, 9, 3, 0.4);
                rivers.lakes = Lake {
                    chance: 0.85,
                    min_start_distance: 0.1,
                    max_start_distance: 0.4,
                    depth: 17,
                    size_min: 40,
                    size_max: 320,
                    min_bank_height: 3,
                    max_bank_height: 7,
                };
                rivers.wetlands = Wetland {
                    chance: 0.9,
                    size_min: 60,
                    size_max: 480,
                };
            }
            4 => {
                world.seed = 0x1_2345_6789;
                continent.continent_scale = 2000;
                continent.continent_skipping = 0.0;
            }
            _ => {}
        }
        (world, rivers)
    }

    /// The oracle's sample points: a 41x41 block grid, then zeros, NaN, infinities and a huge value on each axis.
    fn points() -> Vec<(f32, f32)> {
        let mut points = Vec::new();
        for i in 0..=40 {
            for j in 0..=40 {
                points.push((-9000.0 + 451.0 * i as f32, -8000.0 + 397.0 * j as f32));
            }
        }
        for e in [
            0.0,
            -0.0,
            f32::NAN,
            f32::INFINITY,
            f32::NEG_INFINITY,
            1e20,
            -123.456,
        ] {
            points.push((e, 0.0));
            points.push((700.0, e));
        }
        points
    }

    /// The direct constructor `create_continent` must dispatch to, and its seed draws.
    fn direct(
        kind: ContinentType,
        seed: &mut Seed,
        root: i32,
        world: WorldSettings,
        rivers: RiverSettings,
        registry: &Arc<TerrainRegistry>,
    ) -> Arc<dyn Continent> {
        let levels = Levels::from_properties(world.properties);
        let registry = Arc::clone(registry);
        match kind {
            ContinentType::Multi => Arc::new(SimpleContinentGenerator::multi(
                seed, root, world, rivers, levels, registry,
            )),
            ContinentType::Single => Arc::new(SimpleContinentGenerator::single(
                seed, root, world, rivers, levels, registry,
            )),
            ContinentType::MultiImproved => Arc::new(AdvancedContinentGenerator::new(
                seed, root, world, rivers, levels, registry,
            )),
            ContinentType::Fancy => Arc::new(FancyContinentGenerator::new(
                seed, root, world, rivers, levels, registry,
            )),
        }
    }

    /// The oracle's `samples` fold: edge, land, nearest centre and `apply` at every point.
    fn fold_samples(continent: &dyn Continent, registry: &TerrainRegistry) -> i32 {
        let mut fold = Fold(17);
        for (x, z) in points() {
            fold.f(continent.edge_value(x, z));
            fold.f(continent.land_value(x, z));
            let center = continent.nearest_center(x, z);
            fold.mix(center as i32);
            fold.mix((center as u64 >> 32) as i32);
            let mut cell = Cell::new(registry);
            continent.apply(&mut cell, x, z);
            fold.cell(&cell, registry);
        }
        fold.0
    }

    /// The oracle's `rivers` fold (`Heightmap.applyRivers` with `getRivermap(cell)`) and its distinct maps.
    fn fold_rivers(continent: &dyn Continent, registry: &TerrainRegistry) -> (i32, usize) {
        let mut fold = Fold(17);
        let mut maps: Vec<Arc<Rivermap>> = Vec::new();
        for i in 0..=16 {
            for j in 0..=16 {
                let (x, z) = (-6000.0 + 757.0 * i as f32, -5000.0 + 613.0 * j as f32);
                let mut cell = Cell::new(registry);
                continent.apply(&mut cell, x, z);
                let map = continent.rivermap_for_cell(&cell);
                let again = continent.rivermap(cell.continent_x, cell.continent_z);
                assert!(Arc::ptr_eq(&map, &again), "not cached");
                if !maps.iter().any(|m| Arc::ptr_eq(m, &map)) {
                    maps.push(Arc::clone(&map));
                }
                fold.mix(map.x());
                fold.mix(map.z());
                map.apply(&mut cell, x, z);
                fold.cell(&cell, registry);
            }
        }
        (fold.0, maps.len())
    }

    /// Engine 0.3.0's `ContinentType.create(seed, context)` against `ContinentFactoryOracle.java` (in the
    /// maintainer's `.local/terraforged/oracles/continent_factory/`): every type over five settings variants with
    /// the context's own seed (as `Heightmap` calls it) and once with a separate `Seed(-98765)`. Checks the seed
    /// draws, the edge, land, nearest-centre and `apply` samples, the river stage through `getRivermap(cell)`, and,
    /// with the context's seed, `new Climate(continent, context)` built on the factory's continent: its draws and
    /// `apply`. The factory's continent also samples bit for bit like the generator built directly.
    #[test]
    fn continent_factory_matches_engine() {
        const EXPECTED: &str = "\
MULTI 0 MultiContinentGenerator draws 4 samples 8e8fa961 rivers c050c0b6 maps 5 climate 7 4a9484f5
MULTI 1 MultiContinentGenerator draws 4 samples 6e978a42 rivers 86003e52 maps 3 climate 7 9d4fc691
MULTI 2 MultiContinentGenerator draws 4 samples 6ad5e057 rivers 20c4eea7 maps 17 climate 7 adb424d6
MULTI 3 MultiContinentGenerator draws 4 samples ad2f3f62 rivers 5fc8c36f maps 21 climate 7 a4d40c04
MULTI 4 MultiContinentGenerator draws 4 samples 588ee367 rivers 4f570c00 maps 5 climate 7 e2b84f89
MULTI 5 MultiContinentGenerator draws 4 samples a6ef99d0 rivers 799a6236 maps 4
SINGLE 0 SingleContinentGenerator draws 4 samples f913494c rivers 121c6f05 maps 2 climate 7 56fba388
SINGLE 1 SingleContinentGenerator draws 4 samples cc2a2f0f rivers 5af3b259 maps 2 climate 7 87d84f1d
SINGLE 2 SingleContinentGenerator draws 4 samples ddaf96df rivers d3a2067d maps 2 climate 7 922ef872
SINGLE 3 SingleContinentGenerator draws 4 samples b7a80a24 rivers 36482b82 maps 2 climate 7 22237bf6
SINGLE 4 SingleContinentGenerator draws 4 samples 801c787b rivers b4cd2886 maps 2 climate 7 fdf00d10
SINGLE 5 SingleContinentGenerator draws 4 samples 0c3ccece rivers 1bb98989 maps 2
MULTI_IMPROVED 0 AdvancedContinentGenerator draws 7 samples 136d046e rivers b362c9a3 maps 5 climate 7 facee959
MULTI_IMPROVED 1 AdvancedContinentGenerator draws 7 samples 3df86830 rivers 49d2f0ec maps 2 climate 7 f0b5bcb7
MULTI_IMPROVED 2 AdvancedContinentGenerator draws 7 samples 289773dd rivers 64f9fbed maps 13 climate 7 67576250
MULTI_IMPROVED 3 AdvancedContinentGenerator draws 7 samples 0dca4420 rivers 894293b5 maps 20 climate 7 b161d374
MULTI_IMPROVED 4 AdvancedContinentGenerator draws 7 samples 6f144100 rivers ca03c394 maps 5 climate 7 9df9e0d4
MULTI_IMPROVED 5 AdvancedContinentGenerator draws 7 samples 7c0ead68 rivers f4bd0af4 maps 4
FANCY 0 FancyContinentGenerator draws 4 samples f77943b9 rivers c8135b15 maps 1 climate 7 8cb676e7
FANCY 1 FancyContinentGenerator draws 4 samples e1c35c5c rivers 034c6537 maps 1 climate 7 c16ab1a6
FANCY 2 FancyContinentGenerator draws 4 samples a5ee587a rivers 29aab656 maps 1 climate 7 fdac2282
FANCY 3 FancyContinentGenerator draws 4 samples 43b60827 rivers d151852c maps 1 climate 7 77d07634
FANCY 4 FancyContinentGenerator draws 4 samples 82852aa9 rivers 56829edc maps 1 climate 7 aec72e3a
FANCY 5 FancyContinentGenerator draws 4 samples 47c9dbaf rivers e385b589 maps 1
";
        let names = [
            ("MULTI", "MultiContinentGenerator"),
            ("SINGLE", "SingleContinentGenerator"),
            ("MULTI_IMPROVED", "AdvancedContinentGenerator"),
            ("FANCY", "FancyContinentGenerator"),
        ];
        let mut out = String::new();
        for kind in ContinentType::VALUES {
            for variant in 0..=5 {
                let own = variant == 5;
                let (world, rivers) = settings(kind, if own { 0 } else { variant });
                let registry = Arc::new(TerrainRegistry::new());
                let levels = Levels::from_properties(world.properties);
                // `GeneratorContext.seed` is `new Seed(settings.world.seed)`: the long's low 32 bits.
                let context_seed = &mut Seed::from_long(world.seed);
                let root = context_seed.root();
                let start = if own {
                    Seed::new(-98_765)
                } else {
                    *context_seed
                };
                let mut seed = start;
                let continent = create_continent(
                    world.continent.continent_type,
                    &mut seed,
                    root,
                    world,
                    rivers,
                    levels,
                    Arc::clone(&registry),
                );
                let draws = seed.get().wrapping_sub(start.get());
                let samples = fold_samples(&*continent, &registry);
                let (rivers_fold, maps) = fold_rivers(&*continent, &registry);

                let mut direct_seed = start;
                let built = direct(kind, &mut direct_seed, root, world, rivers, &registry);
                assert_eq!(direct_seed.get(), seed.get());
                assert_eq!(fold_samples(&*built, &registry), samples);
                assert_eq!(fold_rivers(&*built, &registry), (rivers_fold, maps));

                let (name, class) = names[kind.index() as usize];
                out += &format!(
                    "{name} {variant} {class} draws {draws} samples {samples:08x} rivers {rivers_fold:08x} maps {maps}"
                );
                if !own {
                    *context_seed = seed;
                    let before = context_seed.get();
                    let climate = Climate::new(
                        continent.clone(),
                        context_seed,
                        &ClimateSettings::default(),
                        world.control_points,
                        levels,
                        &registry,
                    );
                    let climate_draws = context_seed.get().wrapping_sub(before);
                    let mut fold = Fold(17);
                    for (i, (x, z)) in points().into_iter().enumerate().step_by(3) {
                        let mut cell = Cell::new(&registry);
                        continent.apply(&mut cell, x, z);
                        cell.value = 0.3 + 0.01 * (i % 50) as f32;
                        climate.apply(&mut cell, x, z);
                        fold.cell(&cell, &registry);
                    }
                    out += &format!(" climate {climate_draws} {:08x}", fold.0);
                }
                out.push('\n');
            }
        }
        assert_eq!(out, EXPECTED);
    }

    /// The oracle's river-generator fold: count, continent scale, min edge value and seed; the main and fork
    /// `RiverConfig`s; `LakeConfig`; `WetlandConfig` (each `Variance` as `apply` at 0, 0.5 and 1).
    fn fold_river_generator<C>(g: &BaseRiverGenerator<C>) -> String {
        let mut base = Fold(17);
        base.mix(g.count);
        base.mix(g.continent_scale);
        base.f(g.min_edge_value);
        base.mix(g.seed);
        let river = |r: &RiverConfig| {
            let mut fold = Fold(17);
            fold.mix(r.order);
            fold.mix(i32::from(r.main));
            fold.mix(r.bed_width);
            fold.mix(r.bank_width);
            fold.f(r.bed_height);
            fold.f(r.min_bank_height);
            fold.f(r.max_bank_height);
            fold.mix(r.length);
            fold.mix(r.length2);
            fold.f(r.fade);
            fold.0
        };
        let l = &g.lake;
        let mut lake = Fold(17);
        for v in [
            l.depth,
            l.chance,
            l.size_min,
            l.size_max,
            l.size_range,
            l.bank_min,
            l.bank_max,
            l.distance_min,
            l.distance_max,
        ] {
            lake.f(v);
        }
        let mut wetland = Fold(17);
        wetland.mix(g.wetland.skip_size);
        for t in [0.0, 0.5, 1.0] {
            wetland.f(g.wetland.length.apply(t));
            wetland.f(g.wetland.width.apply(t));
        }
        format!(
            "base {:08x} main {:08x} fork {:08x} lake {:08x} wetland {:08x}",
            base.0,
            river(&g.main),
            river(&g.fork),
            lake.0,
            wetland.0
        )
    }

    /// Engine 0.3.0 with every river setting changed (variant 6) against Engine's defaults (variant 0), from
    /// `ContinentFactoryOracle.java`'s `RIVERS` lines: the river generator each continent builds from the context's
    /// river settings has Engine's count, seed and main, fork, lake and wetland configs bit for bit, and the
    /// factory's continent carves the same cells (`getRivermap(cell)` then `Rivermap.apply`) over a dense grid.
    #[test]
    fn continent_river_settings_match_engine() {
        const EXPECTED: &str = "\
RIVERS MULTI 0 base a22f3fa8 main 39493726 fork 4eacb877 lake 11198ac4 wetland 145827d3 carve 535120f9 masked 768 river 0 lake 0 wetland 0
RIVERS MULTI 6 base a23017a1 main daeee328 fork 6a3669a7 lake efcf61d5 wetland b5f73610 carve 3718bd38 masked 3030 river 0 lake 2 wetland 0
RIVERS SINGLE 0 base a22f3fa8 main 39493726 fork 4eacb877 lake 11198ac4 wetland 145827d3 carve 0ce4f21d masked 1027 river 0 lake 10 wetland 0
RIVERS SINGLE 6 base a23017a1 main daeee328 fork 6a3669a7 lake efcf61d5 wetland b5f73610 carve 232a3c4f masked 1703 river 0 lake 238 wetland 0
RIVERS MULTI_IMPROVED 0 base a22f3fa8 main 39493726 fork 4eacb877 lake 11198ac4 wetland 145827d3 carve 646c4b45 masked 2785 river 0 lake 19 wetland 0
RIVERS MULTI_IMPROVED 6 base a23017a1 main daeee328 fork 6a3669a7 lake efcf61d5 wetland b5f73610 carve ecfb5b90 masked 3736 river 0 lake 50 wetland 0
RIVERS FANCY 0 base a22f3fa8 main 39493726 fork 4eacb877 lake 11198ac4 wetland 145827d3 carve d324601b masked 6428 river 0 lake 51 wetland 0
RIVERS FANCY 6 base a23017a1 main daeee328 fork 6a3669a7 lake efcf61d5 wetland b5f73610 carve eebf5e24 masked 5661 river 0 lake 1106 wetland 0
";
        let names = ["MULTI", "SINGLE", "MULTI_IMPROVED", "FANCY"];
        let mut out = String::new();
        for kind in ContinentType::VALUES {
            for variant in [0, 6] {
                let (world, rivers) = settings(kind, variant);
                let registry = Arc::new(TerrainRegistry::new());
                let levels = Levels::from_properties(world.properties);
                let (mut factory_seed, mut direct_seed) =
                    (Seed::from_long(world.seed), Seed::from_long(world.seed));
                let root = factory_seed.root();
                let continent = create_continent(
                    kind,
                    &mut factory_seed,
                    root,
                    world,
                    rivers,
                    levels,
                    Arc::clone(&registry),
                );
                // The generator each continent's own `river_generator` builds (Engine's river cache holds it).
                let (seed, reg) = (&mut direct_seed, Arc::clone(&registry));
                let generator = match kind {
                    ContinentType::Multi => fold_river_generator(
                        &SimpleContinentGenerator::multi(seed, root, world, rivers, levels, reg)
                            .river_generator()
                            .base,
                    ),
                    ContinentType::Single => fold_river_generator(
                        &SimpleContinentGenerator::single(seed, root, world, rivers, levels, reg)
                            .river_generator()
                            .base,
                    ),
                    ContinentType::MultiImproved => fold_river_generator(
                        &AdvancedContinentGenerator::new(seed, root, world, rivers, levels, reg)
                            .river_generator()
                            .base,
                    ),
                    ContinentType::Fancy => fold_river_generator(
                        FancyContinentGenerator::new(seed, root, world, rivers, levels, reg)
                            .river_generator()
                            .base(),
                    ),
                };
                let terrain = |name| registry.get_by_name(name).expect(name);
                let (river, lake, wetland) =
                    (terrain("river"), terrain("lake"), terrain("wetland"));
                let mut fold = Fold(17);
                let mut counts = [0; 4];
                for i in 0..=150 {
                    for j in 0..=150 {
                        let (x, z) = (-3500.0 + 47.0 * i as f32, -3200.0 + 43.0 * j as f32);
                        let mut cell = Cell::new(&registry);
                        continent.apply(&mut cell, x, z);
                        continent.rivermap_for_cell(&cell).apply(&mut cell, x, z);
                        fold.cell(&cell, &registry);
                        counts[0] += usize::from(cell.river_mask < 1.0);
                        counts[1] += usize::from(cell.terrain.is_same(&river));
                        counts[2] += usize::from(cell.terrain.is_same(&lake));
                        counts[3] += usize::from(cell.terrain.is_same(&wetland));
                    }
                }
                out += &format!(
                    "RIVERS {} {variant} {generator} carve {:08x} masked {} river {} lake {} wetland {}\n",
                    names[kind.index() as usize],
                    fold.0,
                    counts[0],
                    counts[1],
                    counts[2],
                    counts[3]
                );
            }
        }
        assert_eq!(out, EXPECTED);
    }
}
