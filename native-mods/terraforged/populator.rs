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
    terraforged::Module,
    terraforged_engine::{Cell, Terrain, TerrainFormSettings, TerrainRegistry},
};

/// Engine's `Populator`: writes its result into a cell.
///
/// Engine's `Populator` also extends `Module`, so a populator can be used as a noise source; here
/// [`Populator::value`] is its `getValue`. Engine runs `apply` on a pooled thread-local cell; here
/// it runs on a new cell built from `registry`, which must be the world's registry.
pub trait Populator: Send + Sync {
    fn apply(&self, cell: &mut Cell, x: f32, z: f32);

    /// `getValue`: `apply` on a scratch cell, then its `value`.
    fn value(&self, registry: &TerrainRegistry, x: f32, z: f32) -> f32 {
        let mut cell = Cell::new(registry);
        self.apply(&mut cell, x, z);
        cell.value
    }
}

/// The methods of Engine's `TerrainPopulator` class that its subclasses (`ScaledPopulator`, later
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

#[cfg(test)]
mod tests {
    use super::super::{
        terraforged_engine::{BiomeType, Levels, Seed, TerrainSettings},
        terraforged_landforms::LandForms,
    };
    use super::*;
    use std::fmt::Write as _;

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
}
