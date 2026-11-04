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
    use super::super::terraforged_engine::{ContinentShape, WorldSettings};
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
}
