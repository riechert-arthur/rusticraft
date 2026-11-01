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
    terraforged_terrain_blend_util::random,
    terraforged_terrain_data::NoiseSample,
};

pub const DEEP_OCEAN: f32 = 0.1;
pub const SHALLOW_OCEAN: f32 = 0.25;
pub const BEACH: f32 = 0.5;
pub const COAST: f32 = 0.55;
pub const INLAND: f32 = 0.6;
pub const BEACH_SIZE: f32 = 0.005;
pub const SLOPE_STEP: i32 = 24;
pub const MAX_FLAT_GRADIENT: f32 = 0.42;

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

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WetlandSettings {
    pub river_size: f32,
    pub coast_size: f32,
    pub chance: f32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LowlandPool {
    Wet,
    Dry,
}

pub fn lowland_pool(sample: &ClimateSample, wetlands: WetlandSettings) -> LowlandPool {
    let eligible = sample.noise.river_noise < wetlands.river_size
        || sample.noise.continent_noise < wetlands.coast_size;
    let wet = eligible
        && (wetlands.chance >= 1.0 || cell_roll(sample.biome_noise, 4153) < wetlands.chance);
    if wet {
        LowlandPool::Wet
    } else {
        LowlandPool::Dry
    }
}

pub fn uses_mountain_pool(biome_noise: f32, mountain_usage: f32) -> bool {
    if mountain_usage <= 0.0 {
        return false;
    }
    mountain_usage >= 1.0 || cell_roll(biome_noise, 7309) < mountain_usage
}

pub fn is_steep(max_y: i32, west: f32, east: f32, north: f32, south: f32) -> bool {
    let half = 1.max(SLOPE_STEP / 2);
    let dx = (east - west) * max_y as f32;
    let dz = (south - north) * max_y as f32;
    let gradient = dx.abs().max(dz.abs()) / (half * 2) as f32;
    gradient > MAX_FLAT_GRADIENT
}

pub fn freezes(base_temperature: f32, height: i32, sea_level: i32) -> bool {
    let threshold = sea_level.wrapping_add(17);
    let mut temperature = base_temperature;
    if height > threshold {
        temperature -= (height - threshold) as f32 * 0.05 / 40.0;
    }
    temperature < 0.15
}

pub fn keeps_snow_cover(
    normal_temperature: f32,
    highland_temperature: f32,
    height: i32,
    sea_level: i32,
) -> bool {
    freezes(normal_temperature, height, sea_level)
        == freezes(highland_temperature, height, sea_level)
}

pub fn cell_roll(biome_noise: f32, salt: i32) -> f32 {
    let bits = if biome_noise.is_nan() {
        0x7fc0_0000_u32
    } else {
        biome_noise.to_bits()
    } as i32;
    random(hash(bits, salt))
}

fn hash(seed: i32, value: i32) -> i32 {
    let mut hash = seed ^ 31_337_i32.wrapping_mul(value);
    hash = hash
        .wrapping_mul(hash)
        .wrapping_mul(hash)
        .wrapping_mul(60_493);
    (hash >> 13) ^ hash
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

    #[test]
    fn cell_rolls_match_current_java_source() {
        let fixtures = [
            (0.0, 1_015_243_200, 1_063_772_364),
            (0.25, 1_049_933_204, 1_063_146_972),
            (0.5, 1_047_641_752, 1_039_401_820),
            (1.0, 1_055_996_260, 1_029_327_800),
            (f32::from_bits(0x7fc0_1234), 1_053_806_592, 1_063_779_620),
        ];
        for (noise, wetland, mountain) in fixtures {
            assert_eq!(cell_roll(noise, 4153).to_bits(), wetland);
            assert_eq!(cell_roll(noise, 7309).to_bits(), mountain);
        }
    }

    #[test]
    fn wetland_and_mountain_pool_gates_match_current_source() {
        let mut sample = ClimateSample::default();
        sample.biome_noise = 0.5;
        sample.noise.river_noise = 0.5;
        sample.noise.continent_noise = 0.8;
        let settings = WetlandSettings {
            river_size: 0.75,
            coast_size: 0.7,
            chance: 0.3,
        };
        assert_eq!(lowland_pool(&sample, settings), LowlandPool::Wet);
        assert_eq!(
            lowland_pool(
                &sample,
                WetlandSettings {
                    chance: 0.2,
                    ..settings
                }
            ),
            LowlandPool::Dry
        );
        sample.noise.river_noise = 0.8;
        assert_eq!(lowland_pool(&sample, settings), LowlandPool::Dry);
        assert!(uses_mountain_pool(0.5, 0.2));
        assert!(!uses_mountain_pool(0.5, 0.1));
        assert!(!uses_mountain_pool(0.5, 0.0));
        assert!(uses_mountain_pool(0.5, 1.0));
    }

    #[test]
    fn slope_and_snow_gates_match_current_java_source() {
        assert!(!is_steep(480, 0.2, 0.221, 0.4, 0.4));
        assert!(is_steep(480, 0.2, 0.222, 0.4, 0.4));
        assert!(is_steep(384, 0.7, 0.6, 0.1, 0.11));
        assert!(!freezes(0.2, 79, 62));
        assert!(freezes(0.2, 120, 62));
        assert!(!freezes(0.6, 200, 62));
        assert!(freezes(0.1, 79, 62));
        assert!(keeps_snow_cover(0.6, 0.2, 79, 62));
        assert!(!keeps_snow_cover(0.6, 0.2, 120, 62));
    }
}
