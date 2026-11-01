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
    terraforged_biome_map::BiomePools,
    terraforged_biome_sampler::{
        BiomeOverride, ClimateSample, InitialBiomeSettings, SLOPE_STEP, biome_override, is_steep,
        select_initial_biome, uses_mountain_pool,
    },
    terraforged_current_biome_sampler::CurrentBiomeSampler,
    terraforged_noise_generator::NoiseGenerator,
    terraforged_terrain_generator::NoiseGenerator as NoiseGeneratorContract,
};
use std::sync::Arc;

pub struct CurrentSurfaceBiomeSampler<'a, T> {
    climate: CurrentBiomeSampler<'a>,
    noise: &'a NoiseGenerator,
    wet: &'a BiomePools<T>,
    dry: &'a BiomePools<T>,
    highland: &'a BiomePools<T>,
    fallback: Arc<T>,
    settings: InitialBiomeSettings,
}

impl<'a, T> CurrentSurfaceBiomeSampler<'a, T> {
    pub fn new(
        noise: &'a NoiseGenerator,
        wet: &'a BiomePools<T>,
        dry: &'a BiomePools<T>,
        highland: &'a BiomePools<T>,
        fallback: Arc<T>,
        settings: InitialBiomeSettings,
    ) -> Self {
        Self {
            climate: CurrentBiomeSampler::new(noise),
            noise,
            wet,
            dry,
            highland,
            fallback,
            settings,
        }
    }

    pub fn sample(
        &self,
        seed: i32,
        x: i32,
        z: i32,
        base_temperature: impl Fn(&T) -> f32,
        override_biome: impl Fn(BiomeOverride) -> Arc<T>,
    ) -> Arc<T> {
        let sample = self.climate.get_sample(seed, x, z);
        let mountain = uses_mountain_pool(sample.biome_noise, self.settings.mountain_usage);
        let height_noise =
            mountain.then(|| NoiseGeneratorContract::height_noise(self.noise, seed, x, z));
        let steep = mountain && self.is_steep(seed, x, z);
        let height = if steep {
            let levels = self.noise.levels();
            levels.height(levels.scaled_height(height_noise.unwrap()))
        } else {
            0
        };
        resolve_surface_biome(
            &sample,
            self.settings,
            self.wet,
            self.dry,
            self.highland,
            &self.fallback,
            steep,
            height,
            base_temperature,
            override_biome,
        )
    }

    fn is_steep(&self, seed: i32, x: i32, z: i32) -> bool {
        let half = 1.max(SLOPE_STEP / 2);
        let west = NoiseGeneratorContract::height_noise(self.noise, seed, x.wrapping_sub(half), z);
        let east = NoiseGeneratorContract::height_noise(self.noise, seed, x.wrapping_add(half), z);
        let north = NoiseGeneratorContract::height_noise(self.noise, seed, x, z.wrapping_sub(half));
        let south = NoiseGeneratorContract::height_noise(self.noise, seed, x, z.wrapping_add(half));
        is_steep(self.noise.levels().max_y, west, east, north, south)
    }
}

#[allow(clippy::too_many_arguments)]
pub fn resolve_surface_biome<T>(
    sample: &ClimateSample,
    settings: InitialBiomeSettings,
    wet: &BiomePools<T>,
    dry: &BiomePools<T>,
    highland: &BiomePools<T>,
    fallback: &Arc<T>,
    steep: bool,
    height: i32,
    base_temperature: impl Fn(&T) -> f32,
    override_biome: impl Fn(BiomeOverride) -> Arc<T>,
) -> Arc<T> {
    let initial = select_initial_biome(
        sample,
        settings,
        wet,
        dry,
        highland,
        fallback,
        steep,
        height,
        base_temperature,
    );
    match biome_override(sample) {
        BiomeOverride::Input => initial,
        replacement => override_biome(replacement),
    }
}

#[cfg(test)]
mod tests {
    use super::super::{
        terraforged_engine::{BiomeType, TerrainRegistry},
        terraforged_terrain_blend_util::WeightMap,
    };
    use super::*;

    fn pools(value: &'static str) -> BiomePools<&'static str> {
        let mut pools = BiomePools::default();
        for climate in BiomeType::VALUES {
            pools.insert(
                climate,
                Arc::new(WeightMap::new(vec![Arc::new(value)], &[1.0])),
            );
        }
        pools
    }

    fn settings() -> InitialBiomeSettings {
        InitialBiomeSettings {
            wetlands: super::super::terraforged_biome_sampler::WetlandSettings {
                river_size: 0.75,
                coast_size: 0.7,
                chance: 1.0,
            },
            mountain_usage: 1.0,
            sea_level: 62,
        }
    }

    #[test]
    fn overrides_run_after_initial_pool_selection() {
        let wet = pools("wet");
        let dry = pools("dry");
        let highland = pools("highland");
        let fallback = Arc::new("fallback");
        let mut sample = ClimateSample::default();
        sample.noise.continent_noise = 0.1;
        sample.biome_noise = 0.4;
        sample.climate_type = BiomeType::Grassland;
        let selected = resolve_surface_biome(
            &sample,
            settings(),
            &wet,
            &dry,
            &highland,
            &fallback,
            true,
            70,
            |_| 0.8,
            |replacement| {
                Arc::new(match replacement {
                    BiomeOverride::DeepOcean => "deep_ocean",
                    _ => "unexpected",
                })
            },
        );
        assert_eq!(*selected, "deep_ocean");
    }

    #[test]
    fn input_keeps_the_completed_highland_decision() {
        let wet = pools("wet");
        let dry = pools("dry");
        let highland = pools("highland");
        let fallback = Arc::new("fallback");
        let mut sample = ClimateSample::default();
        sample.noise.continent_noise = 0.9;
        sample.noise.river_noise = 1.0;
        sample.noise.terrain_type = TerrainRegistry::new().get_by_id(8);
        sample.biome_noise = 0.4;
        let selected = resolve_surface_biome(
            &sample,
            settings(),
            &wet,
            &dry,
            &highland,
            &fallback,
            true,
            70,
            |_| 0.8,
            |_| Arc::new("unexpected"),
        );
        assert_eq!(*selected, "highland");
    }
}
