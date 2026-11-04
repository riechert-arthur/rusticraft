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
    terraforged_climate::{Climate, ClimateSettings, LandValue},
    terraforged_continent_api::{Continent, create_continent},
    terraforged_engine::{
        Levels, RiverSettings, Seed, TerrainRegistry, TerrainSettings, WorldSettings,
    },
    terraforged_filters::FilterSettings,
};

/// Engine's `Settings`: the world, climate, terrain, river and filter settings, with Engine's defaults.
#[derive(Clone, Copy, Default)]
pub struct GeneratorSettings {
    pub world: WorldSettings,
    pub climate: ClimateSettings,
    pub terrain: TerrainSettings,
    pub rivers: RiverSettings,
    pub filters: FilterSettings,
}

/// Engine's `GeneratorContext`: the world's seed, levels and settings, and (in place of Engine's static
/// `TerrainType` registry) the world's one [`TerrainRegistry`].
///
/// Java shares one mutable `Seed` object between everything built from a context, while [`Seed`] here is `Copy`:
/// every consumer must draw through `&mut context.seed` (as [`GeneratorContext::continent`] and
/// [`GeneratorContext::climate`] do), never through a copy of it, or the draws are lost and later layers get
/// the wrong seeds. The context is deliberately not `Clone`; Engine's `copy`/`split` reset the seed.
///
/// Engine's context also holds a `TerrainProviderFactory` (always `StandardTerrainProvider::new` here) and two lazy
/// suppliers, the `WorldGeneratorFactory` and, on the `create` path, the tile cache built from it. Neither is
/// forced while the context is built, so they draw no seed; they come with the world generator factory layer.
pub struct GeneratorContext {
    pub seed: Seed,
    pub levels: Levels,
    pub settings: GeneratorSettings,
    /// The world's terrain registry; continents, the climate, heightmap caches and tiles compare terrains by
    /// identity with its built-ins, so every layer of the world gets this same instance.
    pub terrain: Arc<TerrainRegistry>,
}

impl GeneratorContext {
    /// Engine's `new GeneratorContext(settings)` (the `create` path, whose world generator and tile cache are
    /// lazy and not ported yet), with a new world registry: the seed is `new Seed(settings.world.seed)` (the
    /// long's low 32 bits, no draws) and the levels are `new Levels(settings.world)`.
    pub fn new(settings: GeneratorSettings) -> Self {
        Self::with_terrain(settings, Arc::new(TerrainRegistry::new()))
    }

    /// Engine's `GeneratorContext.createNoCache(settings)`: the same seed, levels and settings as [`Self::new`]
    /// (only Engine's unforced tile-cache supplier differs).
    pub fn create_no_cache(settings: GeneratorSettings) -> Self {
        Self::new(settings)
    }

    /// [`Self::new`] over the world's existing registry.
    pub fn with_terrain(settings: GeneratorSettings, terrain: Arc<TerrainRegistry>) -> Self {
        Self {
            seed: Seed::from_long(settings.world.seed),
            levels: Levels::from_properties(settings.world.properties),
            settings,
            terrain,
        }
    }

    /// Engine's `copy()`: `split(0)`.
    pub fn copy(&self) -> Self {
        self.split(0)
    }

    /// Engine's `split(offset)`: a new seed at `root + offset` (wrapping, cursor reset to it); the levels,
    /// settings and the registry are shared (the settings by value: Engine shares the object, which nothing
    /// mutates after the context is built).
    pub fn split(&self, offset: i32) -> Self {
        Self {
            seed: self.seed.offset(offset),
            levels: self.levels,
            settings: self.settings,
            terrain: Arc::clone(&self.terrain),
        }
    }

    /// Engine's `seed()`: a *new* seed at the root (`seed.split()`). Draws on it never reach `self.seed`; to
    /// draw from the context's own seed, use `&mut self.seed`.
    pub const fn seed(&self) -> Seed {
        self.seed.split()
    }

    /// Engine's `seed(offset)`: a new seed at `root + offset`, wrapping.
    pub const fn seed_offset(&self, offset: i32) -> Seed {
        self.seed.offset(offset)
    }

    /// `world.continent.continentType.create(context.seed, context)`, as Engine's `Heightmap` calls it: draws
    /// from the context's own seed, takes the river seed from its root and shares the world's registry.
    pub fn continent(&mut self) -> Arc<dyn Continent> {
        let world = self.settings.world;
        let root = self.seed.root();
        create_continent(
            world.continent.continent_type,
            &mut self.seed,
            root,
            world,
            self.settings.rivers,
            self.levels,
            Arc::clone(&self.terrain),
        )
    }

    /// Engine's `new Climate(continent, context)`: draws from the context's own seed and compares terrains with
    /// the world's registry.
    pub fn climate(&mut self, continent: Arc<dyn LandValue>) -> Climate {
        Climate::new(
            continent,
            &mut self.seed,
            &self.settings.climate,
            self.settings.world.control_points,
            self.levels,
            &self.terrain,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::super::terraforged_engine::{Cell, ContinentType};
    use super::super::terraforged_populator::Populator;
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
    }

    fn bits(v: f32) -> String {
        format!("{:08x}", v.to_bits())
    }

    /// Engine 0.3.0, from `GeneratorContextOracle.java` (`.local/terraforged/oracles/generator_context/`): the
    /// seed after `createNoCache` and `new GeneratorContext`, the levels, the draws and cell fields of a
    /// continent and a climate built from the context, and what `copy`/`split`/`seed()` share and duplicate.
    #[test]
    fn generator_context_matches_engine_0_3_0() {
        const EXPECTED: &str = "\
SEED 0 0 0 0 0
SEED 12345 12345 12345 12345 12345
SEED -987654321 -987654321 -987654321 -987654321 -987654321
SEED 4886718345 591751049 591751049 591751049 591751049
SEED -9223372036854775808 0 0 0 0
SEED 9223372036854775807 -1 -1 -1 -1
SEED 4294967295 -1 -1 -1 -1
LEVELS 256 63 256 3b800000 62 64 63 3e7c0000 3e780000 3e7c0000 3f5e37f3
LEVELS 320 80 320 3b4ccccd 79 81 80 3e800000 3e7ccccd 3e800000 3f5e021f
LEVELS 384 62 384 3b2aaaab 61 63 62 3e255555 3e22aaab 3e255555 3f6190b9
LEVELS 0 0 1 3f800000 -1 1 0 00000000 bf800000 00000000 3f733333
LEVELS -5 400 1 3f800000 1 401 400 3f800000 3f800000 3f800000 00000000
LEVELS 1 -1 1 3f800000 -2 0 -1 bf800000 c0000000 bf800000 3f777778
LEVELS 255 63 255 3b808081 62 64 63 3e7cfcfd 3e78f8f9 3e7cfcfd 3f5e2d19
LEVELS 2147483647 2147483647 2147483647 30000000 2147483646 -2147483648 2147483647 3f800000 3f800000 3f800000 00000000
BUILD MULTI continent 4 climate 7 cursor -55544 fold a7128aa1
BUILD SINGLE continent 4 climate 7 cursor -55544 fold f237fec7
BUILD MULTI_IMPROVED continent 7 climate 7 cursor -55541 fold a9a8ebb8
BUILD FANCY continent 4 climate 7 cursor -55544 fold bbf9c7d2
SPLIT copy seed 4242 4243 parent 4247
SPLIT split9 seed 4251 4252 parent 4247
SPLIT split-4242 seed 0 1 parent 4247
SPLIT splitMax seed -2147479407 -2147479406 parent 4247
SEEDS seed() 4242 4243 4242 seed(5) 4247 4247 parent 4247
";
        let mut out = String::new();
        for seed in [
            0,
            12_345,
            -987_654_321,
            0x1_2345_6789,
            i64::MIN,
            i64::MAX,
            0xFFFF_FFFF,
        ] {
            let mut settings = GeneratorSettings::default();
            settings.world.seed = seed;
            let (a, b) = (
                GeneratorContext::create_no_cache(settings),
                GeneratorContext::new(settings),
            );
            out += &format!(
                "SEED {seed} {} {} {} {}\n",
                a.seed.root(),
                a.seed.get(),
                b.seed.root(),
                b.seed.get()
            );
        }
        for (height, sea_level) in [
            (256, 63),
            (320, 80),
            (384, 62),
            (0, 0),
            (-5, 400),
            (1, -1),
            (255, 63),
            (i32::MAX, i32::MAX),
        ] {
            let mut settings = GeneratorSettings::default();
            settings.world.properties.world_height = height;
            settings.world.properties.sea_level = sea_level;
            let l = GeneratorContext::create_no_cache(settings).levels;
            out += &format!(
                "LEVELS {height} {sea_level} {} {} {} {} {} {} {} {} {}\n",
                l.world_height,
                bits(l.unit),
                l.water_y,
                l.ground_level,
                l.water_level,
                bits(l.ground),
                bits(l.water),
                bits(l.ground(0)),
                bits(l.elevation_value(0.9))
            );
        }
        let names = ["MULTI", "SINGLE", "MULTI_IMPROVED", "FANCY"];
        for kind in ContinentType::VALUES {
            let mut settings = GeneratorSettings::default();
            settings.world.seed = -55_555;
            settings.world.continent.continent_type = kind;
            let mut context = GeneratorContext::create_no_cache(settings);
            let start = context.seed.get();
            let continent = context.continent();
            let after_continent = context.seed.get();
            let climate = context.climate(continent.clone());
            let end = context.seed.get();
            let registry = &context.terrain;
            let mut fold = Fold(17);
            for i in 0..=30 {
                for j in 0..=30 {
                    let (x, z) = (-7000.0 + 467.0 * i as f32, -6000.0 + 419.0 * j as f32);
                    let mut cell = Cell::new(registry);
                    continent.apply(&mut cell, x, z);
                    cell.value = 0.2 + 0.013 * ((i * 31 + j) % 60) as f32;
                    climate.apply(&mut cell, x, z);
                    for v in [
                        cell.value,
                        cell.moisture,
                        cell.temperature,
                        cell.biome_region_id,
                        cell.biome_region_edge,
                        cell.macro_biome_id,
                        cell.continent_edge,
                    ] {
                        fold.f(v);
                    }
                    // The climate writes the world registry's own terrains.
                    assert!(cell.terrain.is_same(&registry.get_by_id(cell.terrain.id())));
                    fold.mix(cell.terrain.id());
                    fold.mix(cell.biome as i32);
                }
            }
            out += &format!(
                "BUILD {} continent {} climate {} cursor {end} fold {:08x}\n",
                names[kind.index() as usize],
                after_continent.wrapping_sub(start),
                end.wrapping_sub(after_continent),
                fold.0
            );
        }
        let mut settings = GeneratorSettings::default();
        settings.world.seed = 4_242;
        let mut context = GeneratorContext::new(settings);
        for _ in 0..5 {
            context.seed.next();
        }
        for (name, offset) in [
            ("copy", None),
            ("split9", Some(9)),
            ("split-4242", Some(-4_242)),
            ("splitMax", Some(i32::MAX)),
        ] {
            let mut derived = offset.map_or_else(|| context.copy(), |o| context.split(o));
            derived.seed.next();
            // Shared: the levels, settings and the very same registry.
            assert_eq!(derived.levels, context.levels);
            assert_eq!(derived.settings.world.seed, context.settings.world.seed);
            assert!(Arc::ptr_eq(&derived.terrain, &context.terrain));
            out += &format!(
                "SPLIT {name} seed {} {} parent {}\n",
                derived.seed.root(),
                derived.seed.get(),
                context.seed.get()
            );
        }
        let (mut s0, s1, s5) = (context.seed(), context.seed(), context.seed_offset(5));
        s0.next();
        out += &format!(
            "SEEDS seed() {} {} {} seed(5) {} {} parent {}\n",
            s0.root(),
            s0.get(),
            s1.get(),
            s5.root(),
            s5.get(),
            context.seed.get()
        );
        assert_eq!(out, EXPECTED);
    }

    /// The context's builders draw from its own seed in Engine's order and share its registry: the continent and
    /// climate equal ones built by hand from one shared `&mut Seed`.
    #[test]
    fn context_builders_draw_from_the_context_seed() {
        let mut settings = GeneratorSettings::default();
        settings.world.seed = 31_337;
        settings.world.continent.continent_type = ContinentType::Fancy;
        let mut context = GeneratorContext::new(settings);
        let continent = context.continent();
        let climate = context.climate(continent.clone());

        let world = settings.world;
        let mut seed = Seed::from_long(world.seed);
        let root = seed.root();
        let registry = Arc::clone(&context.terrain);
        let direct = create_continent(
            world.continent.continent_type,
            &mut seed,
            root,
            world,
            settings.rivers,
            context.levels,
            Arc::clone(&registry),
        );
        let direct_climate = Climate::new(
            direct.clone(),
            &mut seed,
            &settings.climate,
            world.control_points,
            context.levels,
            &registry,
        );
        assert_eq!(seed.get(), context.seed.get());
        for (x, z) in [(0.0, 0.0), (-1234.5, 987.0), (40_000.0, -3_333.0)] {
            let (mut a, mut b) = (Cell::new(&registry), Cell::new(&registry));
            continent.apply(&mut a, x, z);
            climate.apply(&mut a, x, z);
            direct.apply(&mut b, x, z);
            direct_climate.apply(&mut b, x, z);
            assert_eq!(a.temperature.to_bits(), b.temperature.to_bits());
            assert_eq!(a.moisture.to_bits(), b.moisture.to_bits());
            assert!(a.terrain.is_same(&b.terrain));
        }
    }
}
