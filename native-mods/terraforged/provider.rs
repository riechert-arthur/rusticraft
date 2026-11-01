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
    terraforged::{Domain, Interpolation, Module, Source, SourceConfig, SourceKind},
    terraforged_engine::{
        Levels, Seed, Terrain, TerrainFormSettings, TerrainRegistry, TerrainSettings,
    },
    terraforged_landforms::LandForms,
    terraforged_populator::{Populator, ScaledPopulator, TerrainPopulator, VolcanoPopulator},
    terraforged_region::RegionConfig,
};
use horizon_core::random::{LegacyRandom, RandomSource};
use std::{collections::HashMap, sync::Arc};

pub struct StandardTerrainProvider {
    mixable: Vec<Arc<dyn Populator>>,
    unmixable: Vec<Arc<dyn Populator>>,
    populators: HashMap<i32, Vec<Arc<dyn Populator>>>,
    seed: Seed,
    levels: Levels,
    landforms: LandForms,
    config: RegionConfig,
    settings: TerrainSettings,
    default_populator: Arc<dyn Populator>,
    terrains: TerrainRegistry,
}

impl StandardTerrainProvider {
    pub fn new(
        seed: Seed,
        levels: Levels,
        settings: TerrainSettings,
        config: RegionConfig,
        default_populator: Arc<dyn Populator>,
    ) -> Self {
        let ground = Module::constant(levels.ground);
        let landforms = LandForms::new(settings, levels, ground);
        let mut provider = Self {
            mixable: Vec::new(),
            unmixable: Vec::new(),
            populators: HashMap::new(),
            seed: seed.offset(settings.general.terrain_seed_offset),
            levels,
            landforms,
            config,
            settings,
            default_populator,
            terrains: TerrainRegistry::new(),
        };
        provider.init();
        provider
    }

    fn init(&mut self) {
        let steppe = self.landforms.steppe(&mut self.seed);
        self.register_form(true, 8, steppe, self.settings.steppe);
        let plains = self.landforms.plains(&mut self.seed);
        self.register_form(true, 8, plains, self.settings.plains);
        let dales = self.landforms.dales(&mut self.seed);
        self.register_form(true, 11, dales, self.settings.dales);
        let hills1 = self.landforms.hills1(&mut self.seed);
        self.register_form(true, 11, hills1, self.settings.hills);
        let hills2 = self.landforms.hills2(&mut self.seed);
        self.register_form(true, 11, hills2, self.settings.hills);
        let torridonian = self.landforms.torridonian(&mut self.seed);
        self.register_form(true, 11, torridonian, self.settings.torridonian);
        let plateau = self.landforms.plateau(&mut self.seed);
        self.register_form(true, 10, plateau, self.settings.plateau);
        let badlands = self.landforms.badlands(&mut self.seed);
        self.register_form(true, 9, badlands, self.settings.badlands);

        let badlands = self.landforms.badlands(&mut self.seed);
        self.register_form(false, 9, badlands, self.settings.badlands);
        let mountains = self.landforms.mountains(&mut self.seed);
        self.register_form(false, 12, mountains, self.settings.mountains);
        let mountains = self.landforms.mountains2(&mut self.seed);
        self.register_form(false, 12, mountains, self.settings.mountains);
        let mountains = self.landforms.mountains3(&mut self.seed);
        self.register_form(false, 12, mountains, self.settings.mountains);

        let volcano = Arc::new(VolcanoPopulator::new(
            &mut self.seed,
            &self.config,
            self.levels,
            self.settings.volcano.weight,
            &self.terrains,
        ));
        self.register_unmixable(volcano);
    }

    fn register_form(
        &mut self,
        mixable: bool,
        terrain_id: i32,
        variance: Module,
        settings: TerrainFormSettings,
    ) {
        let terrain = self.terrains.get_by_id(terrain_id);
        let base = self.landforms.land_base();
        let populator: Arc<dyn Populator> =
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
            };
        if mixable {
            self.register_mixable(populator);
        } else {
            self.register_unmixable(populator);
        }
    }

    pub fn register_mixable(&mut self, populator: Arc<dyn Populator>) {
        let terrain = populator
            .terrain_type()
            .expect("mixable terrain populator has no terrain type");
        self.populators
            .entry(terrain.id())
            .or_default()
            .push(Arc::clone(&populator));
        self.mixable.push(populator);
    }

    pub fn register_unmixable(&mut self, populator: Arc<dyn Populator>) {
        let terrain = populator
            .terrain_type()
            .expect("unmixable terrain populator has no terrain type");
        self.populators
            .entry(terrain.id())
            .or_default()
            .push(Arc::clone(&populator));
        self.unmixable.push(populator);
    }

    pub fn terrain(&self, name: &str) -> Option<Terrain> {
        self.populators.keys().find_map(|id| {
            let terrain = self.terrains.get_by_id(*id);
            terrain.name().eq_ignore_ascii_case(name).then_some(terrain)
        })
    }

    pub fn variant_count(&self, terrain: &Terrain) -> usize {
        self.populators.get(&terrain.id()).map_or(0, Vec::len)
    }

    pub fn populator(&self, terrain: &Terrain, variant: i32) -> &dyn Populator {
        if variant < 0 {
            return self.default_populator.as_ref();
        }
        let Some(populators) = self.populators.get(&terrain.id()) else {
            return self.default_populator.as_ref();
        };
        let index = usize::try_from(variant)
            .unwrap_or(usize::MAX)
            .min(populators.len() - 1);
        populators[index].as_ref()
    }

    pub fn landforms(&self) -> &LandForms {
        &self.landforms
    }

    pub fn terrain_populators(&self) -> Vec<&dyn Populator> {
        self.mixable
            .iter()
            .chain(&self.unmixable)
            .map(Arc::as_ref)
            .collect()
    }

    pub fn populators(&mut self) -> Vec<Arc<dyn Populator>> {
        let mixable: Vec<_> = self
            .mixable
            .iter()
            .filter(|populator| {
                populator
                    .terrain_weight()
                    .is_some_and(|weight| weight > 0.0)
            })
            .cloned()
            .collect();
        let mut result = self.combine_all(&mixable);
        result.extend(self.unmixable.iter().cloned());
        let mut random = LegacyRandom::new(i64::from(self.seed.next()));
        for size in (2..=result.len()).rev() {
            let index = random
                .next_int_bound(size as i32)
                .expect("shuffle bound is positive") as usize;
            result.swap(size - 1, index);
        }
        result
    }

    fn combine_all(&mut self, input: &[Arc<dyn Populator>]) -> Vec<Arc<dyn Populator>> {
        let length = input.len()
            + (1..input.len())
                .map(|index| input.len() - index)
                .sum::<usize>();
        let mut result = Vec::with_capacity(length);
        result.extend(input.iter().cloned());
        for first in 0..input.len() {
            for second in first + 1..input.len() {
                result.push(self.combine(&input[first], &input[second]));
            }
        }
        result
    }

    fn combine(
        &mut self,
        first: &Arc<dyn Populator>,
        second: &Arc<dyn Populator>,
    ) -> Arc<dyn Populator> {
        let first_type = first.terrain_type().expect("terrain populator has no type");
        let second_type = second
            .terrain_type()
            .expect("terrain populator has no type");
        let terrain = self.terrains.register_composite(first_type, second_type);
        let scale = self.config.scale / 2;
        let control = perlin(self.seed.next(), scale, 1).warp(noise_warp(
            self.seed.next(),
            scale / 2,
            2,
            scale as f32 / 2.0,
        ));
        let combined = control
            .blend(
                first
                    .terrain_variance()
                    .expect("terrain populator has no variance")
                    .clone(),
                second
                    .terrain_variance()
                    .expect("terrain populator has no variance")
                    .clone(),
                0.5,
                0.25,
                Interpolation::Linear,
            )
            .clamp(Module::constant(0.0), Module::constant(1.0));
        let weight = (first.terrain_weight().unwrap() + second.terrain_weight().unwrap()) / 2.0;
        Arc::new(TerrainPopulator::new(
            terrain,
            self.landforms.land_base(),
            combined,
            weight,
        ))
    }
}

fn perlin(seed: i32, scale: i32, octaves: usize) -> Module {
    Module::source(Source::new(
        SourceKind::Perlin,
        SourceConfig {
            seed,
            octaves,
            gain: 0.5,
            frequency: 1.0 / scale as f32,
            lacunarity: 2.0,
            interpolation: Interpolation::Curve3,
        },
    ))
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
    use super::*;

    #[test]
    fn standard_provider_matches_engine_0_3_0() {
        let mut settings = TerrainSettings::default();
        settings.general.terrain_seed_offset = 17;
        let levels = Levels::new(256, 63);
        let config = RegionConfig::new(777, 400, perlin(91, 100, 2), perlin(92, 100, 2), 50.5);
        let fallback: Arc<dyn Populator> = Arc::new(TerrainPopulator::new(
            TerrainRegistry::new().get_by_id(0),
            Module::constant(0.0),
            Module::constant(0.123),
            1.0,
        ));
        let mut provider = StandardTerrainProvider::new(
            Seed::from_long(246_813_579),
            levels,
            settings,
            config,
            fallback,
        );
        let registry = TerrainRegistry::new();
        assert_eq!(provider.variant_count(&registry.get_by_id(8)), 2);
        assert_eq!(provider.variant_count(&registry.get_by_id(11)), 4);
        assert_eq!(provider.variant_count(&registry.get_by_id(10)), 1);
        assert_eq!(provider.variant_count(&registry.get_by_id(9)), 2);
        assert_eq!(provider.variant_count(&registry.get_by_id(12)), 3);
        assert_eq!(provider.variant_count(&registry.get_by_id(14)), 1);
        assert_eq!(provider.terrain_populators().len(), 13);
        assert_eq!(provider.terrain("MoUnTaInS").unwrap().id(), 12);

        let mut cell = super::super::terraforged_engine::Cell::default();
        provider
            .populator(&registry.get_by_id(12), -1)
            .apply(&mut cell, 0.0, 0.0);
        assert_eq!(cell.value.to_bits(), 0x3dfbe76d);
        assert_eq!(cell.terrain.id(), 0);
        provider
            .populator(&registry.get_by_id(8), 999)
            .apply(&mut cell, 0.0, 0.0);
        assert_eq!(cell.value.to_bits(), 0x3e90ecc0);
        assert_eq!(cell.terrain.id(), 8);

        let populators = provider.populators();
        assert_eq!(populators.len(), 41);
        let hash = populators.iter().fold(1_i32, |hash, populator| {
            let mut cell = super::super::terraforged_engine::Cell::default();
            populator.apply(&mut cell, 12.25, -98.75);
            hash.wrapping_mul(31)
                .wrapping_add(populator.terrain_type().unwrap().id())
                .wrapping_mul(31)
                .wrapping_add(populator.terrain_weight().unwrap().to_bits() as i32)
                .wrapping_mul(31)
                .wrapping_add(cell.value.to_bits() as i32)
                .wrapping_mul(31)
                .wrapping_add(cell.terrain.id())
        });
        assert_eq!(hash as u32, 0x7e3d577a);
    }
}
