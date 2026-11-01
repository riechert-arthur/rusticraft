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
        CellFunction, DistanceFunction, Domain, EdgeFunction, Interpolation, Module, Source,
        SourceConfig, SourceKind,
    },
    terraforged_engine::{Cell, Levels, Seed, Terrain, TerrainFormSettings, TerrainRegistry},
    terraforged_region::RegionConfig,
};

pub trait Populator {
    fn apply(&self, cell: &mut Cell, x: f32, z: f32);

    fn terrain_weight(&self) -> Option<f32> {
        None
    }

    fn terrain_variance(&self) -> Option<&Module> {
        None
    }

    fn terrain_type(&self) -> Option<&Terrain> {
        None
    }

    fn value(&self, x: f32, z: f32) -> f32 {
        let mut cell = Cell::default();
        self.apply(&mut cell, x, z);
        cell.value
    }
}

pub trait TerrainPopulatorLike: Populator {
    fn weight(&self) -> f32;
    fn variance(&self) -> &Module;
    fn terrain_type(&self) -> &Terrain;
    fn as_config(&self) -> TerrainConfig;
}

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

    pub fn clamp(module: Module) -> Module {
        if module.min_value() < 0.0 || module.max_value() > 1.0 {
            module.clamp(Module::constant(0.0), Module::constant(1.0))
        } else {
            module
        }
    }

    pub fn of(terrain_type: Terrain, variance: Module) -> Self {
        Self::new(terrain_type, Module::constant(0.0), variance, 1.0)
    }

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

    fn terrain_variance(&self) -> Option<&Module> {
        Some(&self.variance)
    }

    fn terrain_type(&self) -> Option<&Terrain> {
        Some(&self.terrain_type)
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

    fn terrain_variance(&self) -> Option<&Module> {
        Some(&self.populator.variance)
    }

    fn terrain_type(&self) -> Option<&Terrain> {
        Some(&self.populator.terrain_type)
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
        TerrainPopulatorLike::terrain_type(&self.populator)
    }

    fn as_config(&self) -> TerrainConfig {
        self.populator.as_config()
    }
}

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
            perlin(seed.next(), 2, 1).map(Module::constant(0.45), Module::constant(0.65));
        let height = Module::cell_source(
            source_config(region.seed, region.scale, 1, 0.5, 2.0),
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
            source_config(region.seed, region.scale, 1, 0.5, 2.0),
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

    pub fn modify_volcano_type(cell: &mut Cell, levels: Levels, terrains: &TerrainRegistry) {
        if cell.terrain.id() == 15
            && (cell.value < levels.water || cell.river_mask < Self::RIVER_MASK)
        {
            cell.terrain = terrains.get_by_id(14);
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

    fn terrain_variance(&self) -> Option<&Module> {
        Some(&self.populator.variance)
    }

    fn terrain_type(&self) -> Option<&Terrain> {
        Some(&self.populator.terrain_type)
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
        TerrainPopulatorLike::terrain_type(&self.populator)
    }

    fn as_config(&self) -> TerrainConfig {
        self.populator.as_config()
    }
}

fn perlin(seed: i32, scale: i32, octaves: usize) -> Module {
    source(SourceKind::Perlin, seed, scale, octaves, 0.5, 2.0)
}

fn source(
    kind: SourceKind,
    seed: i32,
    scale: i32,
    octaves: usize,
    gain: f32,
    lacunarity: f32,
) -> Module {
    Module::source(Source::new(
        kind,
        source_config(seed, scale, octaves, gain, lacunarity),
    ))
}

fn source_config(
    seed: i32,
    scale: i32,
    octaves: usize,
    gain: f32,
    lacunarity: f32,
) -> SourceConfig {
    SourceConfig {
        seed,
        octaves,
        gain,
        frequency: 1.0 / scale as f32,
        lacunarity,
        interpolation: Interpolation::Curve3,
    }
}

fn noise_warp(seed: i32, scale: i32, octaves: usize, strength: f32) -> Domain {
    Domain::domain_warp(
        perlin(seed, scale, octaves),
        perlin(seed.wrapping_add(1), scale, octaves),
        Module::constant(strength),
    )
}

#[cfg(test)]
mod tests {
    use super::super::terraforged_engine::TerrainRegistry;
    use super::*;

    fn run(populator: &dyn TerrainPopulatorLike) -> (u32, i32, u32, u32, u32) {
        let mut cell = Cell::default();
        populator.apply(&mut cell, 12.25, -98.75);
        (
            cell.value.to_bits(),
            cell.terrain.id(),
            populator.weight().to_bits(),
            populator.variance().min_value().to_bits(),
            populator.variance().max_value().to_bits(),
        )
    }

    #[test]
    fn terrain_populators_match_engine_0_3_0() {
        let terrains = TerrainRegistry::new();
        let regular = TerrainPopulator::new(
            terrains.get_by_id(12),
            Module::constant(0.25),
            Module::constant(0.5),
            0.75,
        );
        assert_eq!(
            run(&regular),
            (0x3f400000, 12, 0x3f400000, 0x3f000000, 0x3f000000)
        );
        assert_eq!(regular.value(12.25, -98.75).to_bits(), 0x3f400000);

        let low = TerrainPopulator::new(
            terrains.get_by_id(11),
            Module::constant(-0.4),
            Module::constant(0.1),
            1.0,
        );
        assert_eq!(run(&low).0, 0x00000000);
        let high = TerrainPopulator::new(
            terrains.get_by_id(10),
            Module::constant(0.7),
            Module::constant(0.8),
            1.0,
        );
        assert_eq!(run(&high).0, 0x3f800000);
        let nan = TerrainPopulator::new(
            terrains.get_by_id(8),
            Module::constant(f32::NAN),
            Module::constant(0.0),
            1.0,
        );
        assert_eq!(run(&nan).0, 0x7fc00000);
    }

    #[test]
    fn scaled_and_configured_populators_match_engine_0_3_0() {
        let terrains = TerrainRegistry::new();
        let settings = TerrainFormSettings {
            weight: 0.6,
            base_scale: 1.5,
            vertical_scale: 0.2,
            horizontal_scale: 1.0,
        };
        let scaled = TerrainPopulator::of_with_settings(
            terrains.get_by_id(9),
            Module::constant(0.25),
            Module::constant(0.5),
            settings,
        );
        assert_eq!(
            run(scaled.as_ref()),
            (0x3ef33333, 9, 0x3f19999a, 0x3f000000, 0x3f000000)
        );

        let configured = scaled.as_config().create_populator(Module::constant(0.125));
        assert_eq!(configured.weight().to_bits(), 0x3f19999a);
        assert_eq!(TerrainPopulatorLike::terrain_type(&configured).id(), 9);
        assert_eq!(configured.value(0.0, 0.0).to_bits(), 0x3f200000);

        let clamped = TerrainPopulator::new(
            terrains.get_by_id(8),
            Module::constant(0.0),
            TerrainPopulator::clamp(Module::constant(1.25)),
            1.0,
        );
        assert_eq!(
            run(&clamped),
            (0x3f800000, 8, 0x3f800000, 0x00000000, 0x3f800000)
        );
    }

    #[test]
    fn volcano_populator_matches_engine_0_3_0() {
        let terrains = TerrainRegistry::new();
        let region = RegionConfig::new(777, 400, perlin(91, 100, 2), perlin(92, 100, 2), 50.5);
        let levels = Levels::new(256, 63);
        let mut seed = Seed::new(12_345);
        let volcano = VolcanoPopulator::new(&mut seed, &region, levels, 2.75, &terrains);
        assert_eq!(seed.get(), 12_349);
        assert_eq!(volcano.weight().to_bits(), 0x40300000);
        for ((x, z), value) in [
            ((0.0, 0.0), 0x3ee4ebeb),
            ((12.25, -98.75), 0x3ee42413),
            ((-4096.5, 8192.25), 0x3e8d8c0f),
            ((123456.75, -0.125), 0x3edcbdc2),
        ] {
            let mut cell = Cell::default();
            volcano.apply(&mut cell, x, z);
            assert_eq!(cell.value.to_bits(), value);
            assert_eq!(cell.terrain.id(), 14);
        }
        let mut pipe = Cell::default();
        volcano.apply(&mut pipe, -940.0, -1960.0);
        assert_eq!(pipe.value.to_bits(), 0x3f243fe2);
        assert_eq!(pipe.terrain.id(), 15);

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
            VolcanoPopulator::modify_volcano_type(&mut cell, levels, &terrains);
            assert_eq!(cell.terrain.id(), terrain);
        }
    }
}
