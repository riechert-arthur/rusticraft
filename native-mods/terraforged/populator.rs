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
    terraforged_engine::{Cell, Terrain, TerrainFormSettings},
};

pub trait Populator {
    fn apply(&self, cell: &mut Cell, x: f32, z: f32);

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
        assert_eq!(configured.terrain_type().id(), 9);
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
}
