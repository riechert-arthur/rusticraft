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

use super::terraforged_biome_sampler::{InitialBiomeSettings, WetlandSettings};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MiscellaneousSettings {
    pub erosion_decorator: bool,
    pub natural_snow_decorator: bool,
    pub vanilla_springs: bool,
    pub vanilla_lava_lakes: bool,
    pub vanilla_lava_springs: bool,
    pub smooth_layer_decorator: bool,
    pub plain_stone_erosion: bool,
    pub custom_biome_features: bool,
    pub strata_decorator: bool,
    pub strata_region_size: i32,
    pub ore_compatible_stone_only: bool,
    pub mountain_biome_usage: f32,
    pub deep_caves: bool,
}

impl Default for MiscellaneousSettings {
    fn default() -> Self {
        Self {
            erosion_decorator: true,
            natural_snow_decorator: true,
            vanilla_springs: true,
            vanilla_lava_lakes: true,
            vanilla_lava_springs: true,
            smooth_layer_decorator: true,
            plain_stone_erosion: false,
            custom_biome_features: true,
            strata_decorator: false,
            strata_region_size: 600,
            ore_compatible_stone_only: true,
            mountain_biome_usage: 1.0,
            deep_caves: true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WetlandsSettings {
    pub chance: f32,
    pub river_size: f32,
    pub coast_size: f32,
}

impl Default for WetlandsSettings {
    fn default() -> Self {
        Self {
            chance: 1.0,
            river_size: 0.75,
            coast_size: 0.7,
        }
    }
}

pub fn initial_biome_settings(
    wetlands: WetlandsSettings,
    miscellaneous: MiscellaneousSettings,
    sea_level: i32,
) -> InitialBiomeSettings {
    InitialBiomeSettings {
        wetlands: WetlandSettings {
            river_size: wetlands.river_size,
            coast_size: wetlands.coast_size,
            chance: wetlands.chance,
        },
        mountain_usage: miscellaneous.mountain_biome_usage,
        sea_level,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_miscellaneous_defaults_match_terra_settings() {
        let settings = MiscellaneousSettings::default();
        assert!(settings.erosion_decorator);
        assert!(settings.natural_snow_decorator);
        assert!(settings.vanilla_springs);
        assert!(settings.vanilla_lava_lakes);
        assert!(settings.vanilla_lava_springs);
        assert!(settings.smooth_layer_decorator);
        assert!(!settings.plain_stone_erosion);
        assert!(settings.custom_biome_features);
        assert!(!settings.strata_decorator);
        assert_eq!(settings.strata_region_size, 600);
        assert!(settings.ore_compatible_stone_only);
        assert_eq!(settings.mountain_biome_usage.to_bits(), 1.0_f32.to_bits());
        assert!(settings.deep_caves);
    }

    #[test]
    fn current_wetland_defaults_feed_the_biome_sampler() {
        let settings = initial_biome_settings(
            WetlandsSettings::default(),
            MiscellaneousSettings::default(),
            62,
        );
        assert_eq!(settings.wetlands.chance.to_bits(), 1.0_f32.to_bits());
        assert_eq!(settings.wetlands.river_size.to_bits(), 0.75_f32.to_bits());
        assert_eq!(settings.wetlands.coast_size.to_bits(), 0.7_f32.to_bits());
        assert_eq!(settings.mountain_usage.to_bits(), 1.0_f32.to_bits());
        assert_eq!(settings.sea_level, 62);
    }
}
