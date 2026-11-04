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
    terraforged::{Interpolation, Module},
    terraforged_engine::{
        Levels, Seed, Terrain, TerrainFormSettings, TerrainRegistry, TerrainSettings,
        java_equals_ignore_case,
    },
    terraforged_landforms::{LandForms, noise_warp, perlin},
    terraforged_populator::{Populator, TerrainPopulator, TerrainPopulatorLike, VolcanoPopulator},
    terraforged_region::RegionConfig,
};
use horizon_core::random::{LegacyRandom, RandomSource};
use std::sync::Arc;

/// Engine's `StandardTerrainProvider`: the default landforms, each registered as a mixable or an
/// unmixable terrain populator, and `getPopulators`, which pairs every mixable populator with
/// every later one into a composite terrain and shuffles the lot.
///
/// Engine takes a `GeneratorContext`; here its parts are passed in: the context's (world) seed,
/// its levels and terrain settings. Engine registers composites into its static `TerrainType`
/// registry; here they go into the world's registry. Pass that same registry to [`new`](Self::new)
/// and to every [`populators`](Self::populators) call, so terrains compare by identity
/// (`is_same`) with the world's built-ins as Engine's `==` does.
pub struct StandardTerrainProvider {
    mixable: Vec<Arc<dyn TerrainPopulatorLike>>,
    unmixable: Vec<Arc<dyn TerrainPopulatorLike>>,
    /// Engine's `HashMap<Terrain, List<Populator>>`, keyed by identity (`Terrain` has no
    /// `equals`), in registration order.
    populators: Vec<(Terrain, Vec<Arc<dyn Populator>>)>,
    seed: Seed,
    landforms: LandForms,
    config: RegionConfig,
    default_populator: Arc<dyn Populator>,
}

impl StandardTerrainProvider {
    pub fn new(
        seed: Seed,
        levels: Levels,
        settings: TerrainSettings,
        config: RegionConfig,
        default_populator: Arc<dyn Populator>,
        registry: &TerrainRegistry,
    ) -> Self {
        // `createGroundNoise`: `Source.constant(levels.ground)`.
        let landforms = LandForms::new(settings, levels, Module::constant(levels.ground));
        let mut provider = Self {
            mixable: Vec::new(),
            unmixable: Vec::new(),
            populators: Vec::new(),
            seed: seed.offset(settings.general.terrain_seed_offset),
            landforms,
            config,
            default_populator,
        };
        provider.init(levels, settings, registry);
        provider
    }

    /// Engine's `init`: the landforms in Engine's order, each drawing its seeds in turn.
    fn init(&mut self, levels: Levels, settings: TerrainSettings, registry: &TerrainRegistry) {
        let t = |id| registry.get_by_id(id);
        let (flats, badlands, plateau, hills, mountains) = (t(8), t(9), t(10), t(11), t(12));
        let forms = &self.landforms;
        let seed = &mut self.seed;
        let mixable = [
            (flats.clone(), forms.steppe(seed), settings.steppe),
            (flats, forms.plains(seed), settings.plains),
            (hills.clone(), forms.dales(seed), settings.dales),
            (hills.clone(), forms.hills1(seed), settings.hills),
            (hills.clone(), forms.hills2(seed), settings.hills),
            (hills, forms.torridonian(seed), settings.torridonian),
            (plateau, forms.plateau(seed), settings.plateau),
            (badlands.clone(), forms.badlands(seed), settings.badlands),
        ];
        for (terrain, variance, form) in mixable {
            let populator = self.form(terrain, variance, form);
            self.register_mixable(populator);
        }
        let forms = &self.landforms;
        let seed = &mut self.seed;
        let unmixable = [
            (badlands, forms.badlands(seed), settings.badlands),
            (mountains.clone(), forms.mountains(seed), settings.mountains),
            (
                mountains.clone(),
                forms.mountains2(seed),
                settings.mountains,
            ),
            (mountains, forms.mountains3(seed), settings.mountains),
        ];
        for (terrain, variance, form) in unmixable {
            let populator = self.form(terrain, variance, form);
            self.register_unmixable(populator);
        }
        let volcano = VolcanoPopulator::new(
            &mut self.seed,
            &self.config,
            levels,
            settings.volcano.weight,
            registry,
        );
        self.register_unmixable(Arc::new(volcano));
    }

    /// `TerrainPopulator.of(type, landForms.getLandBase(), variance, settings)`.
    fn form(
        &self,
        terrain: Terrain,
        variance: Module,
        settings: TerrainFormSettings,
    ) -> Arc<dyn TerrainPopulatorLike> {
        let base = self.landforms.land_base();
        Arc::from(TerrainPopulator::of_with_settings(
            terrain, base, variance, settings,
        ))
    }

    fn register(&mut self, populator: &Arc<dyn TerrainPopulatorLike>) {
        let terrain = populator.terrain_type();
        let index = match self.populators.iter().position(|(t, _)| t.is_same(terrain)) {
            Some(index) => index,
            None => {
                self.populators.push((terrain.clone(), Vec::new()));
                self.populators.len() - 1
            }
        };
        self.populators[index].1.push(populator.clone());
    }

    pub fn register_mixable(&mut self, populator: Arc<dyn TerrainPopulatorLike>) {
        self.register(&populator);
        self.mixable.push(populator);
    }

    pub fn register_unmixable(&mut self, populator: Arc<dyn TerrainPopulatorLike>) {
        self.register(&populator);
        self.unmixable.push(populator);
    }

    fn variants(&self, terrain: &Terrain) -> Option<&[Arc<dyn Populator>]> {
        self.populators
            .iter()
            .find(|(t, _)| t.is_same(terrain))
            .map(|(_, list)| list.as_slice())
    }

    /// `getTerrain`: the registered terrain whose name equals `name` ignoring case. Engine
    /// iterates its `HashMap`'s keys; their names are unique ignoring case, so the order does not
    /// matter.
    pub fn terrain(&self, name: &str) -> Option<Terrain> {
        self.populators
            .iter()
            .find(|(terrain, _)| java_equals_ignore_case(terrain.name(), name))
            .map(|(terrain, _)| terrain.clone())
    }

    pub fn variant_count(&self, terrain: &Terrain) -> usize {
        self.variants(terrain).map_or(0, <[_]>::len)
    }

    /// `getPopulator`: the default populator for a negative variant or an unregistered terrain,
    /// else the variant, capped at the last one.
    pub fn populator(&self, terrain: &Terrain, variant: i32) -> &dyn Populator {
        let Ok(variant) = usize::try_from(variant) else {
            return self.default_populator.as_ref();
        };
        match self.variants(terrain) {
            Some(list) => list[variant.min(list.len() - 1)].as_ref(),
            None => self.default_populator.as_ref(),
        }
    }

    pub fn landforms(&self) -> &LandForms {
        &self.landforms
    }

    /// `forEach` / `getTerrainPopulators`: the mixable populators, then the unmixable ones.
    pub fn terrain_populators(&self) -> impl Iterator<Item = &Arc<dyn TerrainPopulatorLike>> {
        self.mixable.iter().chain(&self.unmixable)
    }

    /// `getPopulators`: the mixable populators with a weight above 0, then one composite for each
    /// pair of them (drawing two seeds each and registering its composite terrain in `registry`,
    /// the world's), then the unmixable populators, shuffled by `Collections.shuffle` with a
    /// `java.util.Random` seeded from the next seed. Every call draws new seeds, as in Engine.
    ///
    /// Engine returns a `List<Populator>`; every entry is a `TerrainPopulator`, so this returns
    /// them as [`TerrainPopulatorLike`] (`RegionSelector` takes them upcast to `Arc<dyn Populator>`).
    pub fn populators(
        &mut self,
        registry: &mut TerrainRegistry,
    ) -> Vec<Arc<dyn TerrainPopulatorLike>> {
        let mixable: Vec<_> = self
            .mixable
            .iter()
            .filter(|populator| populator.weight() > 0.0)
            .cloned()
            .collect();
        let mut result: Vec<Arc<dyn TerrainPopulatorLike>> =
            Vec::with_capacity(mixable.len() * (mixable.len() + 1) / 2 + self.unmixable.len());
        result.extend(mixable.iter().cloned());
        for (i, first) in mixable.iter().enumerate() {
            for second in &mixable[i + 1..] {
                result.push(Arc::new(self.combine(
                    first.as_ref(),
                    second.as_ref(),
                    registry,
                )));
            }
        }
        result.extend(self.unmixable.iter().cloned());
        // `Collections.shuffle(list, rnd)` on an `ArrayList`: from the end, swap each slot with
        // `rnd.nextInt(i)`. `new Random(int)` sign-extends the seed.
        let mut random = LegacyRandom::new(i64::from(self.seed.next()));
        for size in (2..=result.len()).rev() {
            let index = random
                .next_int_bound(size as i32)
                .expect("shuffle bound is positive") as usize;
            result.swap(size - 1, index);
        }
        result
    }

    /// `combine(tp1, tp2, seed, config.scale / 2)`: the composite terrain first, then a Perlin
    /// control warped by a Perlin warp blends the two variances.
    fn combine(
        &mut self,
        first: &dyn TerrainPopulatorLike,
        second: &dyn TerrainPopulatorLike,
        registry: &mut TerrainRegistry,
    ) -> TerrainPopulator {
        let terrain = registry.register_composite(first.terrain_type(), second.terrain_type());
        let scale = self.config.scale / 2;
        let control = perlin(self.seed.next(), scale, 1, 2.0);
        // `(double) scale / 2.0`, then `Source.constant`'s float: the same as halving the float.
        let warp = noise_warp(self.seed.next(), scale / 2, 2, scale as f32 / 2.0);
        let combined = control
            .warp(warp)
            .blend(
                first.variance().clone(),
                second.variance().clone(),
                0.5,
                0.25,
                Interpolation::Linear,
            )
            .clamp(Module::constant(0.0), Module::constant(1.0));
        let weight = (first.weight() + second.weight()) / 2.0;
        TerrainPopulator::new(terrain, self.landforms.land_base(), combined, weight)
    }
}

#[cfg(test)]
mod tests {
    use super::super::{
        terraforged::SourceKind, terraforged_engine::Cell, terraforged_landforms::source,
    };
    use super::*;
    use std::fmt::Write as _;

    fn bits(value: f32) -> String {
        format!("{:08x}", value.to_bits())
    }

    fn fnv1a(text: &str) -> u64 {
        text.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
        })
    }

    fn address(populator: &dyn Populator) -> *const () {
        populator as *const dyn Populator as *const ()
    }

    fn list_text(
        out: &mut String,
        name: &str,
        list: &[Arc<dyn TerrainPopulatorLike>],
        registry: &TerrainRegistry,
    ) {
        let xs = [
            0.0_f32, 12.25, -98.75, -4096.5, 8192.25, 123_456.75, -1500.5, 2400.75,
        ];
        writeln!(out, "{name} size={}", list.len()).unwrap();
        for (i, populator) in list.iter().enumerate() {
            let (terrain, weight, variance) = (
                populator.terrain_type(),
                populator.weight(),
                populator.variance(),
            );
            writeln!(
                out,
                "{name} {i} type={}:{} weight={} vmin={} vmax={}",
                terrain.id(),
                terrain.name(),
                bits(weight),
                bits(variance.min_value()),
                bits(variance.max_value()),
            )
            .unwrap();
            let mut samples = String::new();
            for x in xs {
                for z in xs {
                    let mut cell = Cell::new(registry);
                    populator.apply(&mut cell, x, z);
                    write!(samples, " {}:{}", bits(cell.value), cell.terrain.id()).unwrap();
                }
            }
            writeln!(out, "{name} {i} samples{samples}").unwrap();
        }
    }

    /// Rebuilds the text `ProviderOracle <config>` prints from Engine 0.3.0 (TerraForged's
    /// `.local/terraforged/oracles/provider/`): Heightmap's region config and a constant default
    /// populator; config 0 is Engine's default settings, config 1 a terrain seed offset, a zero
    /// and a NaN mixable weight, scaled landforms and a negative long world seed.
    fn oracle_text(config: i32) -> String {
        let mut settings = TerrainSettings::default();
        let world_seed = if config == 0 {
            246_813_579_i64
        } else {
            settings.general.terrain_seed_offset = 17;
            settings.general.terrain_region_size = 777;
            settings.dales.weight = 0.0;
            settings.torridonian.weight = f32::NAN;
            settings.plateau.vertical_scale = 0.8;
            settings.hills.base_scale = 1.25;
            settings.mountains.vertical_scale = 0.7;
            settings.steppe.weight = 3.5;
            -98_765_432_109
        };
        let mut registry = TerrainRegistry::new();
        let world = Seed::from_long(world_seed);
        let mut region_warp = world.offset(8934);
        let simplex = |seed| source(SourceKind::Simplex, seed, 400, 1, 0.5, 2.0);
        let warp_x = simplex(region_warp.next());
        let warp_z = simplex(region_warp.next());
        let region = RegionConfig::new(
            world.offset(789_124).get(),
            settings.general.terrain_region_size,
            warp_x,
            warp_z,
            200.0,
        );
        let fallback: Arc<dyn Populator> = Arc::new(TerrainPopulator::new(
            registry.get_by_id(0),
            Module::constant(0.0),
            Module::constant(0.123),
            1.0,
        ));
        let mut provider = StandardTerrainProvider::new(
            world,
            Levels::new(256, 63),
            settings,
            region,
            fallback.clone(),
            &registry,
        );
        let mut out = String::new();
        writeln!(out, "seed after init={}", provider.seed.get()).unwrap();
        let registered: Vec<_> = provider.terrain_populators().cloned().collect();
        let registered: Vec<_> = registered.iter().collect();
        for (i, populator) in registered.iter().enumerate() {
            writeln!(
                out,
                "registered {i} type={} weight={}",
                populator.terrain_type().id(),
                bits(populator.weight())
            )
            .unwrap();
        }
        for id in 0..16 {
            let terrain = registry.get_by_id(id);
            let mut line = format!("variants {id} count={}", provider.variant_count(&terrain));
            for variant in [-1, 0, 1, 2, 3, 4, 999, i32::MIN, i32::MAX] {
                let found = address(provider.populator(&terrain, variant));
                let described = if found == address(fallback.as_ref()) {
                    "default".to_string()
                } else {
                    registered
                        .iter()
                        .position(|p| address(p.as_ref()) == found)
                        .map_or("?".to_string(), |i| format!("#{i}"))
                };
                write!(line, " {described}").unwrap();
            }
            writeln!(out, "{line}").unwrap();
        }
        let names = [
            "MoUnTaInS",
            "flats",
            "FLATS",
            "hills",
            "volcano",
            "volcano_pipe",
            "none",
            "flats-hills",
            "badlands",
            "plateau",
            "bogus",
        ];
        for name in names {
            let found = provider.terrain(name).map_or("null".to_string(), |t| {
                assert!(t.is_same(&registry.get_by_id(t.id())));
                format!("{}:{}", t.id(), t.name())
            });
            writeln!(out, "getTerrain {name} {found}").unwrap();
        }
        let first = provider.populators(&mut registry);
        list_text(&mut out, "first", &first, &registry);
        writeln!(out, "seed after first={}", provider.seed.get()).unwrap();
        let second = provider.populators(&mut registry);
        list_text(&mut out, "second", &second, &registry);
        writeln!(out, "seed after second={}", provider.seed.get()).unwrap();
        for terrain in registry.terrains() {
            writeln!(out, "registry {} {}", terrain.id(), terrain.name()).unwrap();
        }
        out
    }

    #[test]
    fn default_provider_matches_engine_0_3_0() {
        let text = oracle_text(0);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 231);
        assert_eq!(lines[0], "seed after init=246813655");
        assert_eq!(
            lines[25],
            "variants 11 count=4 default #2 #3 #4 #5 #5 #5 default #5"
        );
        assert_eq!(
            lines[42],
            "first 0 type=8:flats weight=40000000 vmin=bca3d70a vmax=3e020c4b"
        );
        assert_eq!(
            lines[92],
            "first 25 type=16:flats-hills weight=40000000 vmin=00000000 vmax=3f800000"
        );
        assert_eq!(lines[124], "seed after first=246813712");
        assert_eq!(lines[208], "seed after second=246813769");
        assert_eq!(lines[230], "registry 21 badlands-plateau");
        assert_eq!(fnv1a(&text), 0xd98a_6cf4_f165_2539);
    }

    #[test]
    fn configured_provider_matches_engine_0_3_0() {
        let text = oracle_text(1);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 171);
        assert_eq!(lines[0], "seed after init=18815792");
        assert_eq!(lines[1], "registered 0 type=8 weight=40600000");
        assert_eq!(
            lines[42],
            "first 0 type=19:plateau-hills weight=3fe00000 vmin=00000000 vmax=3f800000"
        );
        assert_eq!(lines[94], "seed after first=18815823");
        assert_eq!(lines[148], "seed after second=18815854");
        assert_eq!(fnv1a(&text), 0x8e41_7a26_e74a_7083);
    }

    #[test]
    fn populators_feed_a_region_selector_with_world_terrains() {
        use super::super::terraforged_region::RegionSelector;
        let mut registry = TerrainRegistry::new();
        let simplex = |seed| source(SourceKind::Simplex, seed, 400, 1, 0.5, 2.0);
        let region = RegionConfig::new(1, 1200, simplex(2), simplex(3), 200.0);
        let fallback: Arc<dyn Populator> = Arc::new(TerrainPopulator::new(
            registry.get_by_id(0),
            Module::constant(0.0),
            Module::constant(0.0),
            1.0,
        ));
        let settings = TerrainSettings::default();
        let levels = Levels::new(256, 63);
        let mut provider = StandardTerrainProvider::new(
            Seed::new(5),
            levels,
            settings,
            region,
            fallback,
            &registry,
        );
        let populators = provider.populators(&mut registry);
        let selector = RegionSelector::new(
            populators
                .into_iter()
                .map(|p| p as Arc<dyn Populator>)
                .collect(),
        );
        let mut cell = Cell::new(&registry);
        cell.terrain_region_id = 0.5;
        selector.apply(&mut cell, 100.0, -200.0);
        // The cell's terrain is one of the world registry's own terrains (built-in or composite).
        let own = registry.get_by_id(cell.terrain.id());
        assert!(cell.terrain.is_same(&own));
    }
}
