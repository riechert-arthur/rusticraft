/*
 * MIT License
 *
 * Copyright (c) 2021 TerraForged
 * Copyright (c) 2020 TerraForged
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

// `GeneratorPreset.build` and `NoiseGenerator`'s factories: the generator a TerraForged
// world preset's `levels` describe, over the bundled terrain resources.

use super::{
    terraforged_climate::ClimateSettings,
    terraforged_context::{GeneratorContext, GeneratorSettings},
    terraforged_continent_config::CONTINENT_SAMPLE_SCALE,
    terraforged_continent_noise::ContinentNoise,
    terraforged_current_terrain_resources::{
        decode_current_terrain_noises, decode_current_terrain_types,
    },
    terraforged_engine::TerrainRegistry,
    terraforged_eroded_noise_generator::ErodedNoiseGenerator,
    terraforged_erosion::{ErosionFilterSettings, HeightmapSmoothingSettings},
    terraforged_generator_facade::TerraForgedGenerator,
    terraforged_noise_generator::NoiseGenerator,
    terraforged_terrain_blender::TerrainBlender,
    terraforged_terrain_levels::TerrainLevels,
};
use serde_json::Value;
use std::sync::Arc;

const TERRAIN_OFFSET: i64 = 45_763_218;
const CONTINENT_OFFSET: i64 = 18_749_560;

/// `TerrainLevels.CODEC`: `auto_scale` and `horizontal_scale` are optional.
pub fn terrain_levels(levels: &Value) -> Result<TerrainLevels, String> {
    let int = |name: &str| {
        levels[name]
            .as_i64()
            .and_then(|value| i32::try_from(value).ok())
            .ok_or_else(|| format!("levels.{name} must be an int"))
    };
    Ok(TerrainLevels::new(
        levels["auto_scale"].as_bool().unwrap_or(true),
        levels["horizontal_scale"]
            .as_f64()
            .map_or(1.0, |scale| scale as f32),
        int("min_y")?,
        int("max_y")?,
        int("base_height")?,
        int("sea_level")?,
        int("sea_floor")?,
    ))
}

/// `GeneratorPreset.build`'s noise: terrain noises in registry order (sorted by
/// location), `createLandTerrain`, `createContinentNoise` and `withErosion`.
pub fn build(levels: TerrainLevels) -> Result<TerraForgedGenerator, String> {
    let mut registry = TerrainRegistry::new();
    let types = decode_current_terrain_types(&mut registry).map_err(|error| error.to_string())?;
    let noises = decode_current_terrain_noises(&types).map_err(|error| error.to_string())?;
    let terrains = Arc::new(registry);
    let land = TerrainBlender::new(
        TERRAIN_OFFSET,
        800,
        0.8,
        0.4,
        noises.into_values().collect(),
    );
    let continent = ContinentNoise::new(
        levels,
        GeneratorContext::new(continent_settings(levels)),
        Arc::clone(&terrains),
    );
    let noise = NoiseGenerator::new(levels, continent, land);
    Ok(TerraForgedGenerator::new(
        levels,
        ErodedNoiseGenerator::new(
            noise,
            ErosionFilterSettings::default(),
            HeightmapSmoothingSettings::default(),
        ),
    ))
}

/// `NoiseGenerator.createContinentNoise`'s settings.
pub fn continent_settings(levels: TerrainLevels) -> GeneratorSettings {
    let mut settings = GeneratorSettings::default();
    settings.world.seed = CONTINENT_OFFSET;
    settings.world.properties.sea_level = levels.sea_level;
    settings.world.properties.world_height = levels.max_y;
    settings.climate = ClimateSettings::terraforged();
    settings.world.continent.continent_scale = CONTINENT_SAMPLE_SCALE;
    settings.world.control_points.deep_ocean = 0.05;
    settings.world.control_points.shallow_ocean = 0.3;
    settings.world.control_points.beach = 0.45;
    settings.world.control_points.coast = 0.75;
    settings.world.control_points.inland = 0.8;
    settings
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bundled_preset_builds_a_generator() {
        let preset: Value =
            serde_json::from_str(super::super::terraforged_world_preset::preset_json()).unwrap();
        let levels =
            terrain_levels(&preset["dimensions"]["minecraft:overworld"]["generator"]["levels"])
                .unwrap();
        assert_eq!(
            (levels.min_y, levels.max_y, levels.sea_level),
            (-64, 480, 62)
        );
        let generator = build(levels).unwrap();
        generator.capture_seed(1234);
        let column = generator.base_column(100, -200);
        let _ = column;
        assert!(terrain_levels(&serde_json::json!({"min_y": 0})).is_err());
    }
}
