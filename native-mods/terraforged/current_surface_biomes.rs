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
    terraforged_biome_sampler::{BiomeOverride, biome_override, initial_biome},
    terraforged_current_biome_sampler::CurrentBiomeSampler,
    terraforged_noise_generator::NoiseGenerator,
};
use std::sync::Arc;

// `BiomeSampler.sampleBiome`: climate sample, then the climate pool, then the ocean,
// shore and river overrides.
pub struct CurrentSurfaceBiomeSampler<'a, T> {
    climate: CurrentBiomeSampler<'a>,
    pools: &'a BiomePools<T>,
    fallback: Arc<T>,
}

impl<'a, T> CurrentSurfaceBiomeSampler<'a, T> {
    pub fn new(noise: &'a NoiseGenerator, pools: &'a BiomePools<T>, fallback: Arc<T>) -> Self {
        Self {
            climate: CurrentBiomeSampler::new(noise),
            pools,
            fallback,
        }
    }

    pub fn sample(
        &self,
        seed: i32,
        x: i32,
        z: i32,
        override_biome: &impl Fn(BiomeOverride) -> Arc<T>,
    ) -> Arc<T> {
        let sample = self.climate.get_sample(seed, x, z);
        let initial = initial_biome(self.pools, &sample, &self.fallback);
        match biome_override(&sample) {
            BiomeOverride::Input => initial,
            replacement => override_biome(replacement),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{
        terraforged_biome_sampler::ClimateSample, terraforged_engine::BiomeType,
        terraforged_terrain_blend_util::WeightMap,
    };
    use super::*;

    #[test]
    fn empty_pools_fall_back_and_overrides_win() {
        let mut pools = BiomePools::default();
        pools.insert(
            BiomeType::Grassland,
            Arc::new(WeightMap::new(vec![Arc::new("plains_pool")], &[1.0])),
        );
        pools.insert(BiomeType::Desert, Arc::new(WeightMap::new(Vec::new(), &[])));
        let fallback = Arc::new("plains");
        let mut sample = ClimateSample::default();
        sample.biome_noise = 0.4;
        sample.climate_type = BiomeType::Grassland;
        assert_eq!(*initial_biome(&pools, &sample, &fallback), "plains_pool");
        sample.climate_type = BiomeType::Desert;
        assert_eq!(*initial_biome(&pools, &sample, &fallback), "plains");
        sample.climate_type = BiomeType::Taiga;
        assert_eq!(*initial_biome(&pools, &sample, &fallback), "plains");
        sample.noise.continent_noise = 0.1;
        assert_eq!(biome_override(&sample), BiomeOverride::DeepColdOcean);
    }
}
