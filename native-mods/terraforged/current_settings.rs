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

// Adapted from TerraForged Reimagined (https://github.com/Romoslayer/TerraForged-Reimagined, a24d450): the defaults of
// `TerraSettings.Miscellaneous` and `TerraSettings.Rivers.Wetlands`, which `BiomeSampler.getInitialBiome`
// (`mountainBiomeUsage`) and `lowlandPool` (the wetlands) read. TerraForged 6dd607eb and the 1.18.2-0.3.1 jar have
// neither. MIT License, Copyright (c) 2020 TerraForged, Copyright (c) 2026 Romoslayer; see
// `LICENSE-TERRAFORGED-REIMAGINED`.

use super::terraforged_biome_sampler::WetlandSettings;

/// Reimagined's `TerraSettings.Miscellaneous`, with its defaults. Only `mountain_biome_usage` is read by a ported
/// layer (pass it to `LiveBiomeBindings::surface_sampler`); the others belong to decorators and features that are
/// not ported yet. Reimagined's `@Range`s (`strata_region_size` 100..=2000, `mountain_biome_usage` 0..=1) are only
/// enforced by its `SettingsSerializer` when it reads a saved world, which is not ported: the struct holds whatever
/// it is given, and the biome sampler compares an out-of-range usage the way Java would.
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

/// Reimagined's `TerraSettings.Rivers.Wetlands` defaults: `chance` 1, `riverSize` 0.75, `coastSize` 0.7 (the
/// thresholds `lowlandPool` used before they were settings). Each field's `@Range` is 0..=1, enforced only by the
/// unported `SettingsSerializer`.
impl Default for WetlandSettings {
    fn default() -> Self {
        Self {
            river_size: 0.75,
            coast_size: 0.7,
            chance: 1.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `.local/terraforged/oracles/current_settings/run.sh`: Reimagined a24d450's `TerraSettings` compiled from
    /// source, fields in declaration order with their defaults (floats as bits) and `@Range` bounds (float bits).
    const ORACLE: &str = "\
miscellaneous erosionDecorator boolean true -
miscellaneous naturalSnowDecorator boolean true -
miscellaneous vanillaSprings boolean true -
miscellaneous vanillaLavaLakes boolean true -
miscellaneous vanillaLavaSprings boolean true -
miscellaneous smoothLayerDecorator boolean true -
miscellaneous plainStoneErosion boolean false -
miscellaneous customBiomeFeatures boolean true -
miscellaneous strataDecorator boolean false -
miscellaneous strataRegionSize int 600 42c80000..44fa0000
miscellaneous oreCompatibleStoneOnly boolean true -
miscellaneous mountainBiomeUsage float 3f800000 0..3f800000
miscellaneous deepCaves boolean true -
wetlands chance float 3f800000 0..3f800000
wetlands riverSize float 3f400000 0..3f800000
wetlands coastSize float 3f333333 0..3f800000
";

    fn float(bits: f32) -> String {
        format!("{:x}", bits.to_bits())
    }

    #[test]
    fn settings_defaults_match_reimagined_java() {
        let m = MiscellaneousSettings::default();
        let w = WetlandSettings::default();
        let unit = format!("0..{}", float(1.0));
        let lines = [
            (
                "miscellaneous erosionDecorator boolean",
                m.erosion_decorator.to_string(),
                "-".to_string(),
            ),
            (
                "miscellaneous naturalSnowDecorator boolean",
                m.natural_snow_decorator.to_string(),
                "-".into(),
            ),
            (
                "miscellaneous vanillaSprings boolean",
                m.vanilla_springs.to_string(),
                "-".into(),
            ),
            (
                "miscellaneous vanillaLavaLakes boolean",
                m.vanilla_lava_lakes.to_string(),
                "-".into(),
            ),
            (
                "miscellaneous vanillaLavaSprings boolean",
                m.vanilla_lava_springs.to_string(),
                "-".into(),
            ),
            (
                "miscellaneous smoothLayerDecorator boolean",
                m.smooth_layer_decorator.to_string(),
                "-".into(),
            ),
            (
                "miscellaneous plainStoneErosion boolean",
                m.plain_stone_erosion.to_string(),
                "-".into(),
            ),
            (
                "miscellaneous customBiomeFeatures boolean",
                m.custom_biome_features.to_string(),
                "-".into(),
            ),
            (
                "miscellaneous strataDecorator boolean",
                m.strata_decorator.to_string(),
                "-".into(),
            ),
            (
                "miscellaneous strataRegionSize int",
                m.strata_region_size.to_string(),
                format!("{}..{}", float(100.0), float(2000.0)),
            ),
            (
                "miscellaneous oreCompatibleStoneOnly boolean",
                m.ore_compatible_stone_only.to_string(),
                "-".into(),
            ),
            (
                "miscellaneous mountainBiomeUsage float",
                float(m.mountain_biome_usage),
                unit.clone(),
            ),
            (
                "miscellaneous deepCaves boolean",
                m.deep_caves.to_string(),
                "-".into(),
            ),
            ("wetlands chance float", float(w.chance), unit.clone()),
            (
                "wetlands riverSize float",
                float(w.river_size),
                unit.clone(),
            ),
            ("wetlands coastSize float", float(w.coast_size), unit),
        ];
        let rust: String = lines
            .iter()
            .map(|(field, value, range)| format!("{field} {value} {range}\n"))
            .collect();
        assert_eq!(rust, ORACLE);
    }

    #[test]
    fn wetland_defaults_are_the_thresholds_the_surface_sampler_was_tested_with() {
        assert_eq!(
            WetlandSettings::default(),
            super::super::terraforged_current_surface_biomes::tests::WETLANDS
        );
    }
}
