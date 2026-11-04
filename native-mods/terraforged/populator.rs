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
        CellFunction, DistanceFunction, Domain, EdgeFunction, Interpolation, Module, SourceConfig,
        SourceKind,
    },
    terraforged_engine::{Cell, Levels, Seed, Terrain, TerrainFormSettings, TerrainRegistry},
    terraforged_landforms::{noise_warp, perlin, source},
    terraforged_region::RegionConfig,
};

/// Engine's `Populator`: writes its result into a cell.
///
/// Engine's `Populator` also extends `Module`, so a populator can be used as a noise source; here
/// [`Populator::value`] is its `getValue`. Engine runs `apply` on a pooled thread-local cell; here
/// it runs on a new cell built from `registry`, which must be the world's registry.
pub trait Populator: Send + Sync {
    fn apply(&self, cell: &mut Cell, x: f32, z: f32);

    /// The weight when this is one of Engine's `TerrainPopulator`s (or a subclass:
    /// `ScaledPopulator`, `VolcanoPopulator`), else `None`. `RegionSelector` weights a
    /// populator by it where Engine checks `instanceof TerrainPopulator`, so every
    /// [`TerrainPopulatorLike`] must return `Some(weight())`.
    fn terrain_weight(&self) -> Option<f32> {
        None
    }

    /// `getValue`: `apply` on a scratch cell, then its `value`.
    fn value(&self, registry: &TerrainRegistry, x: f32, z: f32) -> f32 {
        let mut cell = Cell::new(registry);
        self.apply(&mut cell, x, z);
        cell.value
    }
}

/// The methods of Engine's `TerrainPopulator` class that its subclasses (`ScaledPopulator`,
/// `VolcanoPopulator`) inherit; Engine's lists of `TerrainPopulator` hold any of them.
pub trait TerrainPopulatorLike: Populator {
    fn weight(&self) -> f32;
    fn variance(&self) -> &Module;
    fn terrain_type(&self) -> &Terrain;
    fn as_config(&self) -> TerrainConfig;
}

/// Engine's `TerrainConfig`: a populator's terrain, variance noise and weight, without its base.
/// Its data spec (the `Terrain` serialisation with defaults `none`, 1 and `Source.ZERO`) is not
/// ported.
#[derive(Clone)]
pub struct TerrainConfig {
    terrain_type: Terrain,
    noise: Module,
    weight: f32,
}

impl TerrainConfig {
    pub fn new(terrain_type: Terrain, noise: Module, weight: f32) -> Self {
        Self {
            terrain_type,
            noise,
            weight,
        }
    }

    pub fn create_populator(&self, base_noise: Module) -> TerrainPopulator {
        TerrainPopulator::new(
            self.terrain_type.clone(),
            base_noise,
            self.noise.clone(),
            self.weight,
        )
    }
}

/// Engine's `TerrainPopulator`: `value = base + variance` clamped to 0..1 (NaN stays NaN), and
/// `terrain = terrain_type`; it writes no other cell field. `terrain_type` must come from the
/// world's [`TerrainRegistry`] so later `is_same` checks against its built-ins hold.
#[derive(Clone)]
pub struct TerrainPopulator {
    weight: f32,
    terrain_type: Terrain,
    base: Module,
    variance: Module,
}

impl TerrainPopulator {
    pub fn new(terrain_type: Terrain, base: Module, variance: Module, weight: f32) -> Self {
        Self {
            weight,
            terrain_type,
            base,
            variance,
        }
    }

    /// `clamp`: `module.clamp(0, 1)` when its bounds leave 0..1, else `module` itself.
    pub fn clamp(module: Module) -> Module {
        if module.min_value() < 0.0 || module.max_value() > 1.0 {
            module.clamp(Module::constant(0.0), Module::constant(1.0))
        } else {
            module
        }
    }

    /// `of(type, variance)`: base `Source.ZERO`, weight 1.
    pub fn of(terrain_type: Terrain, variance: Module) -> Self {
        Self::new(terrain_type, Module::constant(0.0), variance, 1.0)
    }

    /// `of(type, base, variance, settings)`: a [`ScaledPopulator`] unless both the vertical and
    /// the base scale are exactly 1.
    pub fn of_with_settings(
        terrain_type: Terrain,
        base: Module,
        variance: Module,
        settings: TerrainFormSettings,
    ) -> Box<dyn TerrainPopulatorLike> {
        if settings.vertical_scale == 1.0 && settings.base_scale == 1.0 {
            Box::new(Self::new(terrain_type, base, variance, settings.weight))
        } else {
            Box::new(ScaledPopulator::new(
                terrain_type,
                base,
                variance,
                settings.base_scale,
                settings.vertical_scale,
                settings.weight,
            ))
        }
    }

    #[allow(clippy::manual_clamp)]
    fn apply_value(&self, cell: &mut Cell, base: f32, variance: f32) {
        cell.value = base + variance;
        if cell.value < 0.0 {
            cell.value = 0.0;
        } else if cell.value > 1.0 {
            cell.value = 1.0;
        }
        cell.terrain = self.terrain_type.clone();
    }
}

impl Populator for TerrainPopulator {
    fn apply(&self, cell: &mut Cell, x: f32, z: f32) {
        self.apply_value(cell, self.base.value(x, z), self.variance.value(x, z));
    }

    fn terrain_weight(&self) -> Option<f32> {
        Some(self.weight)
    }
}

impl TerrainPopulatorLike for TerrainPopulator {
    fn weight(&self) -> f32 {
        self.weight
    }

    fn variance(&self) -> &Module {
        &self.variance
    }

    fn terrain_type(&self) -> &Terrain {
        &self.terrain_type
    }

    fn as_config(&self) -> TerrainConfig {
        TerrainConfig::new(
            self.terrain_type.clone(),
            self.variance.clone(),
            self.weight,
        )
    }
}

/// Engine's `ScaledPopulator`: [`TerrainPopulator`] with the base and variance values multiplied
/// by their scales (in `f32`) before the sum. `as_config` keeps only the variance, as in Engine.
#[derive(Clone)]
pub struct ScaledPopulator {
    populator: TerrainPopulator,
    base_scale: f32,
    variance_scale: f32,
}

impl ScaledPopulator {
    pub fn new(
        terrain_type: Terrain,
        base: Module,
        variance: Module,
        base_scale: f32,
        variance_scale: f32,
        weight: f32,
    ) -> Self {
        Self {
            populator: TerrainPopulator::new(terrain_type, base, variance, weight),
            base_scale,
            variance_scale,
        }
    }
}

impl Populator for ScaledPopulator {
    fn apply(&self, cell: &mut Cell, x: f32, z: f32) {
        let base = self.populator.base.value(x, z) * self.base_scale;
        let variance = self.populator.variance.value(x, z) * self.variance_scale;
        self.populator.apply_value(cell, base, variance);
    }

    fn terrain_weight(&self) -> Option<f32> {
        Some(self.populator.weight)
    }
}

impl TerrainPopulatorLike for ScaledPopulator {
    fn weight(&self) -> f32 {
        self.populator.weight()
    }

    fn variance(&self) -> &Module {
        self.populator.variance()
    }

    fn terrain_type(&self) -> &Terrain {
        self.populator.terrain_type()
    }

    fn as_config(&self) -> TerrainConfig {
        self.populator.as_config()
    }
}

/// Engine's `VolcanoPopulator`, a [`TerrainPopulator`] of `volcano` (base and variance 0, so its
/// config and variance are those of a zero populator) that writes only `value` and `terrain`.
/// `value` is the levels' ground plus a cone: an inverted cell-edge distance (the region config's
/// seed, scale and warp) curved, clamped, mapped and warped, scaled by a per-cell height of
/// 0.45..0.65. Above 94% of that height the cone turns down into a throat, and where it is more
/// than 92.5% of the way to the top the terrain becomes `volcano_pipe`; below 0.15 the cone gets
/// ridged lowlands and below 0.45 a fading share of them, both with terrain `volcano`. Between
/// those bands the cell's terrain is left as it was. It draws four seeds, in Engine's order.
/// Its terrains must come from the world's [`TerrainRegistry`].
pub struct VolcanoPopulator {
    populator: TerrainPopulator,
    cone: Module,
    height: Module,
    lowlands: Module,
    inversion_point: f32,
    blend_lower: f32,
    blend_upper: f32,
    blend_range: f32,
    bias: f32,
    inner: Terrain,
    outer: Terrain,
}

impl VolcanoPopulator {
    pub const RIVER_MASK: f32 = 0.85;
    pub const COAST_MASK: f32 = 0.85;

    pub fn new(
        seed: &mut Seed,
        region: &RegionConfig,
        levels: Levels,
        weight: f32,
        terrains: &TerrainRegistry,
    ) -> Self {
        let midpoint = 0.3;
        let range = 0.3;
        let blend_lower = midpoint - range / 2.0;
        let blend_upper = blend_lower + range;
        let blend_range = blend_upper - blend_lower;
        let height_noise =
            perlin(seed.next(), 2, 1, 2.0).map(Module::constant(0.45), Module::constant(0.65));
        let height = Module::cell_source(
            cell_config(region.seed, region.scale),
            1.0,
            CellFunction::NoiseLookup,
            DistanceFunction::Euclidean,
            height_noise,
        )
        .warp(Domain::domain_warp(
            region.warp_x.clone(),
            region.warp_z.clone(),
            Module::constant(region.warp_strength as f32),
        ));
        let cone = Module::cell_edge_source(
            cell_config(region.seed, region.scale),
            1.0,
            EdgeFunction::Distance2Div,
            DistanceFunction::Euclidean,
        )
        .invert()
        .warp(Domain::domain_warp(
            region.warp_x.clone(),
            region.warp_z.clone(),
            Module::constant(region.warp_strength as f32),
        ))
        .power_curve(11.0)
        .clamp(Module::constant(0.475), Module::constant(1.0))
        .map(Module::constant(0.0), Module::constant(1.0))
        .gradient(
            Module::constant(0.0),
            Module::constant(0.5),
            Module::constant(0.5),
        )
        .warp(noise_warp(seed.next(), 15, 2, 10.0))
        .scale(height.clone());
        let lowlands = source(SourceKind::Ridge, seed.next(), 150, 3, 0.975, 2.0)
            .warp(noise_warp(seed.next(), 30, 1, 30.0))
            .scale(Module::constant(0.1));
        let outer = terrains.get_by_id(14);
        let inner = terrains.get_by_id(15);
        Self {
            populator: TerrainPopulator::new(
                outer.clone(),
                Module::constant(0.0),
                Module::constant(0.0),
                weight,
            ),
            cone,
            height,
            lowlands,
            inversion_point: 0.94,
            blend_lower,
            blend_upper,
            blend_range,
            bias: levels.ground,
            inner,
            outer,
        }
    }

    /// `modifyVolcanoType`, which Heightmap runs after the rivers: a `volcano_pipe` cell (the
    /// registry's own, as Engine's `==` compares) below the water level or with a river mask under
    /// 0.85 becomes `volcano`. A NaN value or mask compares false and keeps the pipe.
    pub fn modify_volcano_type(cell: &mut Cell, levels: Levels, terrains: &TerrainRegistry) {
        let builtins = terrains.terrains();
        if cell.terrain.is_same(&builtins[15])
            && (cell.value < levels.water || cell.river_mask < Self::RIVER_MASK)
        {
            cell.terrain = builtins[14].clone();
        }
    }
}

impl Populator for VolcanoPopulator {
    fn apply(&self, cell: &mut Cell, x: f32, z: f32) {
        let mut value = self.cone.value(x, z);
        let limit = self.height.value(x, z);
        let max_height = limit * self.inversion_point;
        if value > max_height {
            let delta = value - max_height;
            let range = limit - max_height;
            let alpha = delta / range;
            if alpha > 0.925 {
                cell.terrain = self.inner.clone();
            }
            value = max_height - max_height / 5.0 * alpha;
        } else if value < self.blend_lower {
            value += self.lowlands.value(x, z);
            cell.terrain = self.outer.clone();
        } else if value < self.blend_upper {
            let alpha = 1.0 - (value - self.blend_lower) / self.blend_range;
            value += self.lowlands.value(x, z) * alpha;
            cell.terrain = self.outer.clone();
        }
        cell.value = self.bias + value;
    }

    fn terrain_weight(&self) -> Option<f32> {
        Some(self.populator.weight)
    }
}

impl TerrainPopulatorLike for VolcanoPopulator {
    fn weight(&self) -> f32 {
        self.populator.weight()
    }

    fn variance(&self) -> &Module {
        self.populator.variance()
    }

    fn terrain_type(&self) -> &Terrain {
        self.populator.terrain_type()
    }

    fn as_config(&self) -> TerrainConfig {
        self.populator.as_config()
    }
}

/// The builder config of `Source.cellNoise` / `Source.cellEdge(seed, scale, ...)`: Engine's defaults
/// otherwise (one octave, gain 0.5, lacunarity 2, Curve3).
fn cell_config(seed: i32, scale: i32) -> SourceConfig {
    SourceConfig {
        seed,
        octaves: 1,
        gain: 0.5,
        frequency: 1.0 / scale as f32,
        lacunarity: 2.0,
        interpolation: Interpolation::Curve3,
    }
}

#[cfg(test)]
mod tests {
    use super::super::{
        terraforged_engine::{BiomeType, Levels, Seed, TerrainSettings},
        terraforged_landforms::LandForms,
        terraforged_region::RegionSelector,
    };
    use super::*;
    use std::fmt::Write as _;
    use std::sync::Arc;

    // Java's `BiomeType` names in id order, for the oracle's text.
    const BIOME_NAMES: [&str; 11] = [
        "TROPICAL_RAINFOREST",
        "SAVANNA",
        "DESERT",
        "TEMPERATE_RAINFOREST",
        "TEMPERATE_FOREST",
        "GRASSLAND",
        "COLD_STEPPE",
        "STEPPE",
        "TAIGA",
        "TUNDRA",
        "ALPINE",
    ];

    fn bits(value: f32) -> String {
        format!("{:08x}", value.to_bits())
    }

    fn dump(cell: &Cell) -> String {
        format!(
            "value={} erosion={} sediment={} gradient={} moisture={} temperature={} continentId={} continentEdge={} trId={} trEdge={} trCenter={} brId={} brEdge={} macro={} river={} cx={} cz={} emask={} terrain={} biome={} absent={}",
            bits(cell.value),
            bits(cell.erosion),
            bits(cell.sediment),
            bits(cell.gradient),
            bits(cell.moisture),
            bits(cell.temperature),
            bits(cell.continent_id),
            bits(cell.continent_edge),
            bits(cell.terrain_region_id),
            bits(cell.terrain_region_edge),
            cell.terrain_region_center,
            bits(cell.biome_region_id),
            bits(cell.biome_region_edge),
            bits(cell.macro_biome_id),
            bits(cell.river_mask),
            cell.continent_x,
            cell.continent_z,
            cell.erosion_mask,
            cell.terrain.id(),
            BIOME_NAMES[cell.biome.id() as usize],
            cell.is_absent(),
        )
    }

    fn filled(registry: &TerrainRegistry) -> Cell {
        let mut cell = Cell::new(registry);
        cell.value = 0.75;
        cell.erosion = 0.25;
        cell.sediment = 0.125;
        cell.gradient = 0.875;
        cell.moisture = 0.2;
        cell.temperature = 0.8;
        cell.continent_id = 0.1;
        cell.continent_edge = 0.9;
        cell.terrain_region_id = 0.3;
        cell.terrain_region_edge = 0.7;
        cell.terrain_region_center = 123_456_789;
        cell.biome_region_id = 0.4;
        cell.biome_region_edge = 0.6;
        cell.macro_biome_id = 0.55;
        cell.river_mask = 0.45;
        cell.continent_x = -123;
        cell.continent_z = 456;
        cell.erosion_mask = true;
        cell.terrain = registry.get_by_id(14);
        cell.biome = BiomeType::Alpine;
        cell
    }

    /// The oracle's lines for one populator: its metadata, then a fresh and a filled cell after
    /// `apply`. `same` checks that both cells hold `expected` itself (`is_same`).
    #[allow(clippy::too_many_arguments)]
    fn run(
        out: &mut String,
        registry: &TerrainRegistry,
        name: &str,
        class: &str,
        populator: &dyn TerrainPopulatorLike,
        expected: &Terrain,
        x: f32,
        z: f32,
    ) {
        let mut fresh = Cell::new(registry);
        populator.apply(&mut fresh, x, z);
        let mut full = filled(registry);
        populator.apply(&mut full, x, z);
        let same = fresh.terrain.is_same(expected) && full.terrain.is_same(expected);
        writeln!(
            out,
            "{name} class={class} weight={} vmin={} vmax={} same={same} getValue={}",
            bits(populator.weight()),
            bits(populator.variance().min_value()),
            bits(populator.variance().max_value()),
            bits(populator.value(registry, x, z)),
        )
        .unwrap();
        writeln!(out, "{name} fresh {}", dump(&fresh)).unwrap();
        writeln!(out, "{name} filled {}", dump(&full)).unwrap();
    }

    fn settings(weight: f32, base_scale: f32, vertical_scale: f32) -> TerrainFormSettings {
        TerrainFormSettings {
            base_scale,
            ..TerrainFormSettings::new(weight, vertical_scale, 1.0)
        }
    }

    fn class(base_scale: f32, vertical_scale: f32) -> &'static str {
        if base_scale == 1.0 && vertical_scale == 1.0 {
            "TerrainPopulator"
        } else {
            "ScaledPopulator"
        }
    }

    fn fnv1a(text: &str) -> u64 {
        text.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
        })
    }

    /// Rebuilds the text `PopulatorOracle` prints from Engine 0.3.0 (TerraForged's
    /// `.local/terraforged/oracles/populator/`): every cell field after `apply` on a fresh and on a
    /// fully set cell, for constant inputs (clamping, NaN, -inf, `of`, settings, `as_config`) and
    /// for real hills and mountains landforms on the land base.
    fn oracle_text() -> String {
        let registry = TerrainRegistry::new();
        let t = |id| registry.get_by_id(id);
        let c = Module::constant;
        let (x, z) = (12.25, -98.75);
        let mut out = String::new();
        let plain = "TerrainPopulator";
        let cases: [(&str, i32, f32, f32, f32); 7] = [
            ("regular", 12, 0.25, 0.5, 0.75),
            ("low", 11, -0.4, 0.1, 1.0),
            ("high", 10, 0.7, 0.8, 1.0),
            ("one", 10, 0.25, 0.75, 1.0),
            ("nanBase", 8, f32::NAN, 0.0, 1.0),
            ("nanVar", 8, 0.25, f32::NAN, 1.0),
            ("negInf", 8, f32::NEG_INFINITY, 0.5, 1.0),
        ];
        for (name, id, base, variance, weight) in cases {
            let populator = TerrainPopulator::new(t(id), c(base), c(variance), weight);
            run(&mut out, &registry, name, plain, &populator, &t(id), x, z);
        }
        let of = TerrainPopulator::of(t(9), c(0.375));
        run(&mut out, &registry, "of", plain, &of, &t(9), x, z);
        let scaled_cases: [(&str, i32, f32, f32, f32, f32, f32); 5] = [
            ("settingsUnit", 11, 0.25, 0.5, 2.0, 1.0, 1.0),
            ("settingsBase", 11, 0.25, 0.5, 1.0, 0.5, 1.0),
            ("scaled", 9, 0.25, 0.5, 0.6, 1.5, 0.2),
            ("scaledHigh", 9, 0.75, 0.5, 1.0, 2.0, 3.0),
            ("scaledNeg", 9, 0.25, 0.5, 1.0, -1.0, 0.1),
        ];
        let mut scaled = None;
        for (name, id, base, variance, weight, base_scale, vertical) in scaled_cases {
            let populator = TerrainPopulator::of_with_settings(
                t(id),
                c(base),
                c(variance),
                settings(weight, base_scale, vertical),
            );
            let class = class(base_scale, vertical);
            run(
                &mut out,
                &registry,
                name,
                class,
                populator.as_ref(),
                &t(id),
                x,
                z,
            );
            if name == "scaled" {
                scaled = Some(populator);
            }
        }
        let config = scaled.unwrap().as_config().create_populator(c(0.125));
        run(&mut out, &registry, "config", plain, &config, &t(9), x, z);
        for value in [1.25_f64, 0.5, -0.1, 0.0] {
            let module = TerrainPopulator::clamp(c(value as f32));
            writeln!(
                out,
                "clamp {value:?} min={} max={} v={}",
                bits(module.min_value()),
                bits(module.max_value()),
                bits(module.value(x, z)),
            )
            .unwrap();
        }

        let terrain_settings = TerrainSettings::default();
        let forms = LandForms::new(terrain_settings, Levels::new(256, 63), c(0.0));
        let mut seed = Seed::new(12_345);
        let hills = TerrainPopulator::of_with_settings(
            t(11),
            forms.land_base(),
            forms.hills1(&mut seed),
            terrain_settings.hills,
        );
        let mountains = TerrainPopulator::of_with_settings(
            t(12),
            forms.land_base(),
            forms.mountains(&mut seed),
            settings(2.5, 1.5, 0.8),
        );
        let xs = [0.0_f32, 12.25, -98.75, -4096.5, 8192.25, 123_456.75];
        for px in xs {
            for pz in xs {
                let hills_name = format!("hills {px:?} {pz:?}");
                let mountains_name = format!("mountains {px:?} {pz:?}");
                let (hills, mountains) = (hills.as_ref(), mountains.as_ref());
                run(
                    &mut out,
                    &registry,
                    &hills_name,
                    plain,
                    hills,
                    &t(11),
                    px,
                    pz,
                );
                let scaled = "ScaledPopulator";
                run(
                    &mut out,
                    &registry,
                    &mountains_name,
                    scaled,
                    mountains,
                    &t(12),
                    px,
                    pz,
                );
            }
        }
        out
    }

    #[test]
    fn populators_write_value_and_terrain_like_engine_0_3_0() {
        let text = oracle_text();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 262);
        // Spot checks against the oracle's lines; the hash covers all 262.
        assert_eq!(
            lines[0],
            "regular class=TerrainPopulator weight=3f400000 vmin=3f000000 vmax=3f000000 same=true getValue=3f400000"
        );
        assert_eq!(
            lines[2],
            "regular filled value=3f400000 erosion=3e800000 sediment=3e000000 gradient=3f600000 moisture=3e4ccccd temperature=3f4ccccd continentId=3dcccccd continentEdge=3f666666 trId=3e99999a trEdge=3f333333 trCenter=123456789 brId=3ecccccd brEdge=3f19999a macro=3f0ccccd river=3ee66666 cx=-123 cz=456 emask=true terrain=12 biome=ALPINE absent=false"
        );
        assert_eq!(
            lines[46],
            "hills 0.0 0.0 class=TerrainPopulator weight=40000000 vmin=00000000 vmax=3f16872c same=true getValue=3e16872c"
        );
        assert_eq!(
            lines[49],
            "mountains 0.0 0.0 class=ScaledPopulator weight=40200000 vmin=00000000 vmax=3f2f9db2 same=true getValue=3eaa69f5"
        );
        assert_eq!(fnv1a(&text), 0xcc7b_1581_6f0c_afc3);
    }

    /// The oracle's name for a cell's terrain: Java's `==` against `TerrainType`'s built-ins.
    fn identity(registry: &TerrainRegistry, cell: &Cell) -> &'static str {
        [
            (14, "volcano"),
            (15, "pipe"),
            (0, "none"),
            (12, "mountains"),
        ]
        .into_iter()
        .find(|&(id, _)| cell.terrain.is_same(&registry.get_by_id(id)))
        .map_or("other", |(_, name)| name)
    }

    /// The oracle's lines for one point: `getValue` and both cells' terrains, then a fresh cell and
    /// a fully set one (holding `mountains`, so an unchanged terrain shows) after `apply`. Where
    /// Engine's cell lookup throws (a NullPointerException, for non-finite coordinates) the port
    /// panics, which is the oracle's `throws` line. Returns the filled cell's terrain identity.
    fn volcano_point(
        out: &mut String,
        registry: &TerrainRegistry,
        name: &str,
        volcano: &VolcanoPopulator,
        x: f32,
        z: f32,
    ) -> &'static str {
        let prefix = format!("{name} {} {}", bits(x), bits(z));
        let probe = std::panic::catch_unwind(|| volcano.value(registry, x, z));
        if probe.is_err() {
            writeln!(out, "{prefix} throws").unwrap();
            return "throws";
        }
        let mut fresh = Cell::new(registry);
        volcano.apply(&mut fresh, x, z);
        let mut full = filled(registry);
        full.terrain = registry.get_by_id(12);
        volcano.apply(&mut full, x, z);
        writeln!(
            out,
            "{prefix} getValue={} ids={},{}",
            bits(volcano.value(registry, x, z)),
            identity(registry, &fresh),
            identity(registry, &full),
        )
        .unwrap();
        writeln!(out, "{prefix} fresh {}", dump(&fresh)).unwrap();
        writeln!(out, "{prefix} filled {}", dump(&full)).unwrap();
        identity(registry, &full)
    }

    #[allow(clippy::too_many_arguments)]
    fn volcano_run(
        out: &mut String,
        registry: &TerrainRegistry,
        name: &str,
        mut seed: Seed,
        region: &RegionConfig,
        levels: Levels,
        weight: f32,
        (step, origin_x, origin_z): (f32, f32, f32),
        (dense, pipe_x, pipe_z): (f32, f32, f32),
    ) {
        let volcano = VolcanoPopulator::new(&mut seed, region, levels, weight, registry);
        writeln!(
            out,
            "{name} seed after={} weight={} type={} same={} vmin={} vmax={}",
            seed.get(),
            bits(volcano.weight()),
            volcano.terrain_type().id(),
            volcano.terrain_type().is_same(&registry.get_by_id(14)),
            bits(volcano.variance().min_value()),
            bits(volcano.variance().max_value()),
        )
        .unwrap();
        let mut counts = [0; 3];
        let mut count = |identity: &str| match identity {
            "pipe" => counts[0] += 1,
            "volcano" => counts[1] += 1,
            "throws" => {}
            _ => counts[2] += 1,
        };
        for i in -20..=20 {
            for j in -20..=20 {
                let (x, z) = (origin_x + i as f32 * step, origin_z + j as f32 * step);
                count(volcano_point(out, registry, name, &volcano, x, z));
            }
        }
        let pipe_name = format!("{name}-pipe");
        for i in -10..=10 {
            for j in -10..=10 {
                let (x, z) = (pipe_x + i as f32 * dense, pipe_z + j as f32 * dense);
                count(volcano_point(out, registry, &pipe_name, &volcano, x, z));
            }
        }
        let special = [
            0.0,
            -0.0,
            1.0e9,
            -3.0e7,
            f32::NAN,
            f32::INFINITY,
            1.4e-45,
            123_456.75,
        ];
        for x in special {
            for z in special {
                count(volcano_point(out, registry, name, &volcano, x, z));
            }
        }
        let [pipe, volcano, kept] = counts;
        writeln!(
            out,
            "{name} counts pipe={pipe} volcano={volcano} kept={kept}"
        )
        .unwrap();
    }

    fn simplex(seed: i32, scale: i32) -> Module {
        source(SourceKind::Simplex, seed, scale, 1, 0.5, 2.0)
    }

    /// Rebuilds the text `VolcanoOracle` prints from Engine 0.3.0 (TerraForged's
    /// `.local/terraforged/oracles/volcano/`): seed draws, weight and every cell field after `apply`
    /// over a grid and a dense window around a pipe, for Heightmap's region config, a small-scale
    /// one and an overflowing seed with ModTerrains' swapped `Levels`; `modifyVolcanoType` around
    /// the water level and the 0.85 river mask; and a `RegionSelector` weighting volcanoes.
    fn volcano_oracle_text() -> String {
        let registry = TerrainRegistry::new();
        let mut out = String::new();
        let levels = Levels::new(256, 63);
        let world = Seed::new(12_345);
        let mut region_warp = world.offset(8934);
        let warp_x = simplex(region_warp.next(), 400);
        let warp_z = simplex(region_warp.next(), 400);
        let heightmap = RegionConfig::new(world.offset(789_124).get(), 1200, warp_x, warp_z, 200.0);
        volcano_run(
            &mut out,
            &registry,
            "heightmap",
            world.offset(0),
            &heightmap,
            levels,
            5.0,
            (150.25, 0.0, 0.0),
            (2.5, 1502.5, -150.25),
        );
        let small = RegionConfig::new(
            777,
            400,
            perlin(91, 100, 2, 2.0),
            perlin(92, 100, 2, 2.0),
            50.5,
        );
        volcano_run(
            &mut out,
            &registry,
            "small",
            Seed::new(12_345),
            &small,
            levels,
            2.75,
            (37.5, -940.0, -1960.0),
            (1.25, -940.0, -1960.0),
        );
        let wrap = RegionConfig::new(
            i32::MIN,
            300,
            simplex(i32::MAX, 200),
            simplex(i32::MIN, 200),
            75.0,
        );
        volcano_run(
            &mut out,
            &registry,
            "wrap",
            Seed::new(i32::MAX - 1),
            &wrap,
            Levels::new(63, 255),
            0.0,
            (23.125, 5000.5, -7000.25),
            (30.5, 5000.5, -7000.25),
        );

        for levels in [levels, Levels::new(63, 255)] {
            let water = levels.water;
            let values = [
                water,
                water.next_down(),
                water.next_up(),
                0.1,
                0.9,
                f32::NAN,
            ];
            let masks = [0.85_f32, 0.85_f32.next_down(), 1.0, 0.0, f32::NAN];
            for (t, id) in [15, 14, 12].into_iter().enumerate() {
                for value in values {
                    for mask in masks {
                        let mut cell = filled(&registry);
                        cell.terrain = registry.get_by_id(id);
                        cell.value = value;
                        cell.river_mask = mask;
                        VolcanoPopulator::modify_volcano_type(&mut cell, levels, &registry);
                        writeln!(
                            out,
                            "modify {} {t} {} {} {} {}",
                            bits(water),
                            bits(value),
                            bits(mask),
                            identity(&registry, &cell),
                            dump(&cell),
                        )
                        .unwrap();
                    }
                }
            }
        }

        let mut seed = Seed::new(4242);
        let constant = |terrain, value, weight| -> Arc<dyn Populator> {
            Arc::new(TerrainPopulator::new(
                registry.get_by_id(terrain),
                Module::constant(0.0),
                Module::constant(value),
                weight,
            ))
        };
        let list: Vec<Arc<dyn Populator>> = vec![
            constant(8, 0.2, 1.5),
            Arc::new(VolcanoPopulator::new(
                &mut seed, &small, levels, 5.0, &registry,
            )),
            constant(11, 0.4, 2.5),
            Arc::new(VolcanoPopulator::new(
                &mut seed, &heightmap, levels, 0.5, &registry,
            )),
        ];
        let selector = RegionSelector::new(list.clone());
        let nodes: String = (0..19)
            .map(|i| {
                let node = selector.get(i as f32 / 18.0) as *const dyn Populator as *const ();
                let index = list
                    .iter()
                    .position(|p| Arc::as_ptr(p) as *const () == node)
                    .unwrap();
                format!(" {index}")
            })
            .collect();
        writeln!(out, "selector nodes=19{nodes}").unwrap();
        for id in [0.0, 0.2, 0.45, 0.75, 1.0] {
            let mut cell = Cell::new(&registry);
            cell.terrain_region_id = id;
            selector.apply(&mut cell, -940.0, -1960.0);
            let identity = identity(&registry, &cell);
            writeln!(out, "selector {} {identity} {}", bits(id), dump(&cell)).unwrap();
        }
        out
    }

    #[test]
    fn volcano_populator_matches_engine_0_3_0() {
        let text = volcano_oracle_text();
        let summary: Vec<&str> = text
            .lines()
            .filter(|line| line.contains("seed after=") || line.contains("counts "))
            .collect();
        assert_eq!(
            summary,
            [
                "heightmap seed after=12349 weight=40a00000 type=14 same=true vmin=00000000 vmax=00000000",
                "heightmap counts pipe=62 volcano=1716 kept=380",
                "small seed after=12349 weight=40300000 type=14 same=true vmin=00000000 vmax=00000000",
                "small counts pipe=41 volcano=1696 kept=421",
                "wrap seed after=-2147483646 weight=00000000 type=14 same=true vmin=00000000 vmax=00000000",
                "wrap counts pipe=0 volcano=2137 kept=21",
            ]
        );
        // The weighted list: a volcano weighs 5 (Engine's `instanceof TerrainPopulator`), not 1.
        assert!(text.contains("\nselector nodes=19 0 0 0 1 1 1 1 1 1 1 1 1 1 2 2 2 2 2 3\n"));
        assert!(text.contains("\nsmall c46b0000 c4f50000 getValue=3f243fe2 ids=pipe,pipe\n"));
        assert_eq!(text.lines().count(), 19_698);
        assert_eq!(text.len(), 5_028_189);
        assert_eq!(fnv1a(&text), 0x129b_6273_a118_513f);
    }
}
