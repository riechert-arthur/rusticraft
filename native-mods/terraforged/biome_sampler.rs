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
    terraforged_engine::{BiomeType, TerrainProperties},
    terraforged_terrain_data::NoiseSample,
};

pub const DEEP_OCEAN: f32 = 0.1;
pub const SHALLOW_OCEAN: f32 = 0.25;
pub const BEACH: f32 = 0.5;
pub const COAST: f32 = 0.55;
pub const INLAND: f32 = 0.6;
pub const BEACH_SIZE: f32 = 0.005;

#[derive(Clone)]
pub struct ClimateSample {
    pub noise: NoiseSample,
    pub biome_noise: f32,
    pub biome_edge_noise: f32,
    pub moisture: f32,
    pub temperature: f32,
    pub climate_type: BiomeType,
}

impl ClimateSample {
    pub fn reset(&mut self) -> &mut Self {
        self.noise.reset();
        self
    }
}

impl Default for ClimateSample {
    fn default() -> Self {
        Self {
            noise: NoiseSample::default(),
            biome_noise: 0.0,
            biome_edge_noise: 0.0,
            moisture: 0.0,
            temperature: 0.0,
            climate_type: BiomeType::Grassland,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BiomeOverride {
    Input,
    DeepColdOcean,
    DeepFrozenOcean,
    DeepLukewarmOcean,
    DeepOcean,
    ColdOcean,
    FrozenOcean,
    WarmOcean,
    Ocean,
    SnowyBeach,
    StonyShore,
    Beach,
    FrozenRiver,
    River,
}

pub fn biome_override(sample: &ClimateSample) -> BiomeOverride {
    let climate = sample.climate_type;
    if sample.noise.continent_noise <= SHALLOW_OCEAN {
        return match climate {
            BiomeType::Taiga | BiomeType::ColdSteppe => BiomeOverride::DeepColdOcean,
            BiomeType::Tundra => BiomeOverride::DeepFrozenOcean,
            BiomeType::Desert | BiomeType::Savanna | BiomeType::TropicalRainforest => {
                BiomeOverride::DeepLukewarmOcean
            }
            _ => BiomeOverride::DeepOcean,
        };
    }
    if sample.noise.continent_noise <= BEACH {
        return match climate {
            BiomeType::Taiga | BiomeType::ColdSteppe => BiomeOverride::ColdOcean,
            BiomeType::Tundra => BiomeOverride::FrozenOcean,
            BiomeType::Desert | BiomeType::Savanna | BiomeType::TropicalRainforest => {
                BiomeOverride::WarmOcean
            }
            _ => BiomeOverride::Ocean,
        };
    }
    if sample.noise.continent_noise <= BEACH + BEACH_SIZE {
        return match climate {
            BiomeType::Tundra => BiomeOverride::SnowyBeach,
            BiomeType::ColdSteppe => BiomeOverride::StonyShore,
            _ => BiomeOverride::Beach,
        };
    }
    if (sample.noise.terrain_type.is_river() || sample.noise.terrain_type.is_lake())
        && sample.noise.river_noise == 0.0
    {
        return if climate == BiomeType::Tundra {
            BiomeOverride::FrozenRiver
        } else {
            BiomeOverride::River
        };
    }
    BiomeOverride::Input
}

#[cfg(test)]
mod tests {
    use super::super::terraforged_engine::TerrainRegistry;
    use super::*;

    fn sample(continent: f32, climate: BiomeType) -> ClimateSample {
        let mut sample = ClimateSample::default();
        sample.noise.continent_noise = continent;
        sample.climate_type = climate;
        sample
    }

    #[test]
    fn biome_overrides_match_terraforged_source() {
        assert_eq!(
            biome_override(&sample(SHALLOW_OCEAN, BiomeType::Taiga)),
            BiomeOverride::DeepColdOcean
        );
        assert_eq!(
            biome_override(&sample(SHALLOW_OCEAN, BiomeType::Tundra)),
            BiomeOverride::DeepFrozenOcean
        );
        assert_eq!(
            biome_override(&sample(SHALLOW_OCEAN, BiomeType::Desert)),
            BiomeOverride::DeepLukewarmOcean
        );
        assert_eq!(
            biome_override(&sample(SHALLOW_OCEAN, BiomeType::Grassland)),
            BiomeOverride::DeepOcean
        );
        assert_eq!(
            biome_override(&sample(BEACH, BiomeType::ColdSteppe)),
            BiomeOverride::ColdOcean
        );
        assert_eq!(
            biome_override(&sample(BEACH, BiomeType::Tundra)),
            BiomeOverride::FrozenOcean
        );
        assert_eq!(
            biome_override(&sample(BEACH, BiomeType::Savanna)),
            BiomeOverride::WarmOcean
        );
        assert_eq!(
            biome_override(&sample(BEACH, BiomeType::Alpine)),
            BiomeOverride::Ocean
        );
        assert_eq!(
            biome_override(&sample(BEACH + BEACH_SIZE, BiomeType::Tundra)),
            BiomeOverride::SnowyBeach
        );
        assert_eq!(
            biome_override(&sample(BEACH + BEACH_SIZE, BiomeType::ColdSteppe)),
            BiomeOverride::StonyShore
        );
        assert_eq!(
            biome_override(&sample(BEACH + BEACH_SIZE, BiomeType::Grassland)),
            BiomeOverride::Beach
        );
    }

    #[test]
    fn river_and_reset_behavior_match_terraforged_source() {
        let terrains = TerrainRegistry::new();
        let mut river = sample(0.75, BiomeType::Grassland);
        river.noise.terrain_type = terrains.get_by_id(5);
        river.noise.river_noise = 0.0;
        assert_eq!(biome_override(&river), BiomeOverride::River);
        river.climate_type = BiomeType::Tundra;
        assert_eq!(biome_override(&river), BiomeOverride::FrozenRiver);
        river.noise.river_noise = f32::MIN_POSITIVE;
        assert_eq!(biome_override(&river), BiomeOverride::Input);
        river.noise.terrain_type = terrains.get_by_id(6);
        river.noise.river_noise = 0.0;
        assert_eq!(biome_override(&river), BiomeOverride::FrozenRiver);

        river.biome_noise = 0.7;
        river.biome_edge_noise = 0.4;
        river.moisture = 0.3;
        river.temperature = 0.2;
        river.reset();
        assert_eq!(river.noise.continent_noise, 1.0);
        assert_eq!(river.noise.river_noise, 1.0);
        assert_eq!(river.biome_noise, 0.7);
        assert_eq!(river.biome_edge_noise, 0.4);
        assert_eq!(river.moisture, 0.3);
        assert_eq!(river.temperature, 0.2);
        assert_eq!(river.climate_type, BiomeType::Tundra);
    }
}
