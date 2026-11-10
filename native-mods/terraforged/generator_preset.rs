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

// TerraForged 6dd607eb (https://github.com/TerraForged/TerraForged/tree/6dd607ebfd41a3b7274e090b1f2cf565d53d1d44, MIT
// License, Copyright (c) 2021 TerraForged): `Generator.CODEC`'s `levels` (`TerrainLevels.CODEC`), `GeneratorPreset.build`
// and `NoiseGenerator`'s factories (`createLandTerrain`, `createContinentNoise`, and `withErosion`'s Engine 0.3.0 filter
// settings); the erosion itself is TerraForged Reimagined's region erosion (`ErodedNoiseGenerator`, see
// `eroded_noise_generator.rs`).

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
    terraforged_flow_rivers::FlowRivers,
    terraforged_generator_facade::TerraForgedGenerator,
    terraforged_noise_generator::{CONTINENT_OFFSET, NoiseGenerator, TERRAIN_OFFSET},
    terraforged_terrain_blender::TerrainBlender,
    terraforged_terrain_levels::TerrainLevels,
};
use serde_json::Value;
use std::sync::Arc;

/// `Generator.CODEC`'s `levels` (`TerrainLevels.CODEC`): absent, `TerrainLevels.DEFAULT`; present, a map whose
/// `min_y`, `max_y`, `base_height`, `sea_level` and `sea_floor` are required ints in `Limits`' ranges (`intRange`)
/// and whose `auto_scale` (default true) and `horizontal_scale` (default 1, `floatRange(0, 10)`) are optional; the
/// constructor then clamps them against each other. A present field of the wrong kind or out of range is an `Err`
/// (26.3's `optionalFieldOf` is strict). Numbers read as `JsonOps` gives them to `Codec.INT`/`FLOAT`
/// (`intValue`/`floatValue`; a boolean counts as 1 or 0, and a boolean field takes a number's `byteValue() != 0`).
pub fn terrain_levels(levels: Option<&Value>) -> Result<TerrainLevels, String> {
    let Some(levels) = levels else {
        return Ok(TerrainLevels::new(
            true,
            TerrainLevels::DEFAULT_SCALE,
            TerrainLevels::DEFAULT_MIN_Y,
            TerrainLevels::DEFAULT_MAX_Y,
            TerrainLevels::DEFAULT_BASE_HEIGHT,
            TerrainLevels::DEFAULT_SEA_LEVEL,
            TerrainLevels::DEFAULT_SEA_FLOOR,
        ));
    };
    if !levels.is_object() {
        return Err("levels must be a map".to_owned());
    }
    let int = |name: &str, min: i32, max: i32| {
        let value = levels
            .get(name)
            .ok_or_else(|| format!("levels.{name} is missing"))
            .and_then(|value| {
                json_int(value).ok_or_else(|| format!("levels.{name} must be a number"))
            })?;
        if (min..=max).contains(&value) {
            Ok(value)
        } else {
            Err(format!("levels.{name} {value} is outside {min}..={max}"))
        }
    };
    let auto_scale = match levels.get("auto_scale") {
        None => true,
        Some(Value::Bool(value)) => *value,
        Some(value) => json_int(value)
            .map(|value| value as i8 != 0)
            .ok_or("levels.auto_scale must be a boolean")?,
    };
    let horizontal_scale = match levels.get("horizontal_scale") {
        None => TerrainLevels::DEFAULT_SCALE,
        Some(value) => {
            let scale = match value {
                Value::Bool(value) => f32::from(u8::from(*value)),
                value => value
                    .as_f64()
                    .ok_or("levels.horizontal_scale must be a number")?
                    as f32,
            };
            if !(0.0..=10.0).contains(&scale) {
                return Err(format!("levels.horizontal_scale {scale} is outside 0..=10"));
            }
            scale
        }
    };
    Ok(TerrainLevels::new(
        auto_scale,
        horizontal_scale,
        int("min_y", TerrainLevels::MIN_MIN_Y, TerrainLevels::MAX_MIN_Y)?,
        int("max_y", TerrainLevels::MIN_MAX_Y, TerrainLevels::MAX_MAX_Y)?,
        int(
            "base_height",
            TerrainLevels::MAX_MIN_Y,
            TerrainLevels::MAX_MAX_Y,
        )?,
        int(
            "sea_level",
            TerrainLevels::MIN_SEA_LEVEL,
            TerrainLevels::MAX_SEA_LEVEL,
        )?,
        int(
            "sea_floor",
            TerrainLevels::MIN_SEA_FLOOR,
            TerrainLevels::MAX_SEA_FLOOR,
        )?,
    ))
}

/// `Number.intValue()` of a JSON number as Gson parses it (an integer keeps its low 32 bits, a fraction is cut
/// toward zero first), or 1/0 for a boolean (`JsonOps.getNumberValue`); `None` for anything else.
fn json_int(value: &Value) -> Option<i32> {
    match value {
        Value::Bool(value) => Some(i32::from(*value)),
        Value::Number(number) => number
            .as_i64()
            .or_else(|| number.as_u64().map(|value| value as i64))
            .or_else(|| number.as_f64().map(|value| value.trunc() as i64))
            .map(|value| value as i32),
        _ => None,
    }
}

/// The `height_scale` setting over the world's `levels` (an `Err`-free mapping of the decoded [`TerrainLevels`]): the
/// land's height above sea level and the oceans' depth below it, times `scale`, at the original's horizontal
/// frequency (`auto_scale` would widen the landforms with the range; here they keep their width and get steeper).
/// TerraForged's noise levels put a land column at `sea_level + base_height * base + (max_y - sea_level -
/// base_height) * terrain` and an ocean floor at `sea_floor + depth * (sea_level - sea_floor)`, so `base_height` (the
/// base's range above the sea) is scaled, and `max_y` and `sea_floor` are stretched around the sea level, the floor
/// no lower than y 0 (`TerrainLevels::MIN_SEA_FLOOR`, the codec's bound). `scale` 1 returns `levels` unchanged.
pub fn scale_heights(levels: TerrainLevels, scale: f32) -> TerrainLevels {
    if scale == 1.0 {
        return levels;
    }
    let sea = levels.sea_level;
    let times = |range: i32| (range as f32 * scale).round() as i32;
    let mut scaled = TerrainLevels::new(
        levels.noise_levels.auto,
        levels.noise_levels.scale,
        levels.min_y,
        sea + times(levels.max_y - sea),
        times(levels.base_height),
        sea,
        (sea - times(sea - levels.sea_floor)).max(TerrainLevels::MIN_SEA_FLOOR),
    );
    scaled.noise_levels.frequency = levels.noise_levels.frequency;
    scaled
}

/// `GeneratorPreset.build`'s noise: `new NoiseGenerator(levels, terrain).withErosion()` over the bundled terrain
/// noises in registry order (`TERRAINS.entries`, sorted by location), with `createLandTerrain`'s blender
/// (`TerrainBlender(TERRAIN_OFFSET, 800, 0.8F, 0.4F, terrain)`) and `createContinentNoise`'s continent. The terrain
/// types, the continent and the blender's terrains share one registry, Engine 0.3.0's built-ins
/// ([`TerrainRegistry::new`], as 6dd607eb links Engine 0.3.0), so every identity test sees the same built-ins. `Err` where a bundled resource does not decode.
/// With Horizon's `flow_rivers` setting (read here, as the world's generators are built), the noise also carves the
/// flow-routed rivers, over a drainage map that samples a second copy of the same noise (with the coasts and glacial
/// troughs of `coastal_landforms` and `glacial_landforms`, as the chunks have them).
pub fn build(levels: TerrainLevels) -> Result<TerraForgedGenerator, String> {
    let mut noise = build_noise_generator(levels)?;
    let settings = super::terraforged_settings::current();
    if settings.flow_rivers {
        noise = noise.with_flow_rivers(FlowRivers::with_settings(
            build_noise_generator(levels)?,
            settings,
        ));
    }
    Ok(TerraForgedGenerator::new(
        levels,
        ErodedNoiseGenerator::new(
            noise,
            erosion_settings(),
            HeightmapSmoothingSettings::default(),
        ),
    ))
}

/// [`build`]'s un-eroded noise, without Horizon's settings.
fn build_noise_generator(levels: TerrainLevels) -> Result<NoiseGenerator, String> {
    let mut registry = TerrainRegistry::new();
    let types = decode_current_terrain_types(&mut registry).map_err(|error| error.to_string())?;
    let noises = decode_current_terrain_noises(&types).map_err(|error| error.to_string())?;
    let context = GeneratorContext::with_terrain(continent_settings(levels), Arc::new(registry));
    let land = TerrainBlender::new(
        TERRAIN_OFFSET,
        800,
        0.8,
        0.4,
        noises.into_values().collect(),
    );
    let continent = ContinentNoise::new(levels, context);
    Ok(NoiseGenerator::new(levels, continent, land))
}

/// `withErosion`'s filter settings: 6dd607eb's `ErodedNoiseGenerator` takes Engine 0.3.0's `new FilterSettings.Erosion()`
/// (lifetime 12, volume and velocity 0.7, erosion and deposit rates 0.5) and sets `dropletsPerChunk = 350`. Not
/// Reimagined's `TerraSettings.Erosion` defaults ([`ErosionFilterSettings::default`]: lifetime 25). The region erosion
/// that runs them is Reimagined's; its heightmap smoothing stays at its default, off (6dd607eb has none).
pub fn erosion_settings() -> ErosionFilterSettings {
    ErosionFilterSettings {
        droplets_per_chunk: 350,
        droplet_lifetime: 12,
        droplet_volume: 0.7,
        droplet_velocity: 0.7,
        erosion_rate: 0.5,
        deposit_rate: 0.5,
    }
}

/// `NoiseGenerator.createContinentNoise`'s settings (Java builds its `GeneratorContext` before it sets the continent
/// scale and control points, but the context reads them from the same `Settings` object later, so they apply).
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
    use super::super::terraforged_generator_facade::{ColumnBlock, TerraHeightmap};
    use super::*;
    use serde_json::json;

    fn fields(levels: TerrainLevels) -> (bool, u32, i32, i32, i32, i32, i32) {
        (
            levels.noise_levels.auto,
            levels.noise_levels.scale.to_bits(),
            levels.min_y,
            levels.max_y,
            levels.base_height,
            levels.sea_level,
            levels.sea_floor,
        )
    }

    #[test]
    fn levels_decode_like_the_terrain_levels_codec() {
        let preset: Value =
            serde_json::from_str(super::super::terraforged_world_preset::preset_json()).unwrap();
        let bundled = terrain_levels(Some(
            &preset["dimensions"]["minecraft:overworld"]["generator"]["levels"],
        ))
        .unwrap();
        assert_eq!(
            fields(bundled),
            (true, 1.0f32.to_bits(), -64, 480, 128, 62, 22)
        );
        // `optionalFieldOf("levels", DEFAULT)`: the defaults are the bundled preset's.
        assert_eq!(fields(terrain_levels(None).unwrap()), fields(bundled));
        let base = json!({"min_y": -32, "max_y": 256, "base_height": 100, "sea_level": 63, "sea_floor": 30});
        assert_eq!(
            fields(terrain_levels(Some(&base)).unwrap()),
            (true, 1.0f32.to_bits(), -32, 256, 100, 63, 30)
        );
        let with = |name: &str, value: Value| {
            let mut levels = base.clone();
            levels[name] = value;
            terrain_levels(Some(&levels))
        };
        assert_eq!(fields(with("auto_scale", json!(false)).unwrap()).0, false);
        assert_eq!(
            fields(with("horizontal_scale", json!(2.5)).unwrap()).1,
            2.5f32.to_bits()
        );
        // `Number.intValue()` truncates; booleans are numbers to JsonOps.
        assert_eq!(fields(with("sea_level", json!(63.9)).unwrap()).5, 63);
        assert_eq!(fields(with("sea_floor", json!(true)).unwrap()).6, 1);
        // The constructor clamps the fields against each other: the base height to [sea level, max y].
        assert_eq!(fields(with("base_height", json!(0)).unwrap()).4, 63);
        // intRange / floatRange and missing or mistyped fields are errors.
        for (name, value) in [
            ("min_y", json!(1)),
            ("min_y", json!(-2033)),
            ("max_y", json!(127)),
            ("max_y", json!(4065)),
            ("sea_level", json!(31)),
            ("sea_floor", json!(-1)),
            ("base_height", json!(-1)),
            ("horizontal_scale", json!(10.5)),
            ("horizontal_scale", json!(-0.5)),
            ("horizontal_scale", json!("1")),
            ("auto_scale", json!("true")),
            ("max_y", json!("480")),
            ("max_y", Value::Null),
        ] {
            assert!(with(name, value.clone()).is_err(), "{name} {value}");
        }
        assert!(terrain_levels(Some(&json!({"min_y": 0}))).is_err());
        assert!(terrain_levels(Some(&json!([]))).is_err());
    }

    /// `oracles/generator_preset/` (`run.sh`): Engine 0.3.0's `FilterSettings.Erosion` from the jar, with
    /// `dropletsPerChunk = 350` as 6dd607eb's `ErodedNoiseGenerator` sets it.
    const JAVA_EROSION: &str = "droplets=350 lifetime=12 volume=3f333333 velocity=3f333333 erosion=3f000000 deposit=3f000000";

    #[test]
    fn erosion_settings_are_6dd607eb_with_erosion() {
        let s = erosion_settings();
        let rust = format!(
            "droplets={} lifetime={} volume={:08x} velocity={:08x} erosion={:08x} deposit={:08x}",
            s.droplets_per_chunk,
            s.droplet_lifetime,
            s.droplet_volume.to_bits(),
            s.droplet_velocity.to_bits(),
            s.erosion_rate.to_bits(),
            s.deposit_rate.to_bits()
        );
        assert_eq!(rust, JAVA_EROSION);
        assert!(HeightmapSmoothingSettings::default().iterations <= 0);
    }

    #[test]
    fn the_bundled_preset_builds_one_registry_generator() {
        let generator = build(terrain_levels(None).unwrap()).unwrap();
        generator.capture_seed(1234);
        // Every column's terrain is one of the continent registry's own: the blender's terrains were decoded into
        // it, so its built-ins (`river`, `deep_ocean`...) and the land's types compare by identity.
        let terrains = Arc::clone(&generator.noise_generator().continent().context().terrain);
        let data = generator.chunk_data(generator.seed(), 6, -12);
        let data = data.read();
        for z in 0..16 {
            for x in 0..16 {
                let terrain = data.terrain().get(x, z).unwrap();
                assert!(
                    terrains
                        .get_by_name(terrain.name())
                        .is_some_and(|own| own.is_same(terrain)),
                    "{}",
                    terrain.name()
                );
            }
        }
        let column = generator.base_column(100, -200, -64, 384).unwrap();
        let height = generator
            .base_height(100, -200, TerraHeightmap::OceanFloorWorldgen)
            .unwrap();
        assert_eq!(column.min_y, -64);
        assert!(height > 0 && height < 320);
        assert_eq!(column.block(height - 1), Some(ColumnBlock::Stone));
        assert_ne!(column.block(height), Some(ColumnBlock::Stone));
    }

    #[test]
    fn height_scale_stretches_heights_around_sea_level() {
        use super::super::terraforged_noise_generator::HEIGHT_MULTIPLIER;
        use super::super::terraforged_settings::HEIGHT_SCALES;
        let original = terrain_levels(None).unwrap();
        assert_eq!(scale_heights(original, 1.0), original);
        let frequency = original.noise_levels.frequency.to_bits();
        let fields = |scale| {
            let levels = scale_heights(original, scale);
            (
                levels.max_y,
                levels.base_height,
                levels.sea_level,
                levels.sea_floor,
                levels.min_y,
                levels.noise_levels.frequency.to_bits(),
            )
        };
        assert_eq!(fields(1.85), (835, 237, 62, 0, -64, frequency));
        assert_eq!(fields(1.5), (689, 192, 62, 2, -64, frequency));
        assert_eq!(fields(0.5), (271, 64, 62, 42, -64, frequency));
        // The highest land the noise can make (base 1, terrain 1, times the height multiplier) stays under the
        // 1024-block overworld's top at the highest scale, and the base range above the sea at the lowest.
        let reach = |levels: TerrainLevels| {
            levels.sea_level as f32
                + levels.base_height as f32
                + HEIGHT_MULTIPLIER * (levels.max_y - levels.sea_level - levels.base_height) as f32
        };
        assert_eq!(reach(original), 538.0);
        let highest = scale_heights(original, *HEIGHT_SCALES.end());
        assert!(reach(highest) < 960.0, "{}", reach(highest));
        let lowest = scale_heights(original, *HEIGHT_SCALES.start());
        assert_eq!(lowest.base_height, 64);
        // Every height above the sea, and every depth below it, scales with the setting: the same un-eroded noise
        // (one sample per column) at the same position, within the rounding of the scaled levels.
        let heights = |levels: TerrainLevels| {
            let generator = build(levels).unwrap();
            generator.capture_seed(98765);
            let noise = generator.noise_generator();
            let max_y = levels.max_y as f32;
            (0..600)
                .map(|i| {
                    let (x, z) = ((i % 30 - 15) * 400 + 7, (i / 30 - 10) * 400 - 3);
                    noise.height_noise(generator.seed(), x, z).unwrap() * max_y
                })
                .collect::<Vec<_>>()
        };
        let one = heights(original);
        let (land, sea) = (
            one.iter().filter(|h| **h >= 62.0).count(),
            one.iter().filter(|h| **h < 62.0).count(),
        );
        assert!(land > 200 && sea > 200, "{land} {sea}");
        for scale in [*HEIGHT_SCALES.start(), 1.5, *HEIGHT_SCALES.end()] {
            // Above 1.55 the floor stops at y 0, so depths scale by 62 / 40 only.
            let depth_scale = scale.min(62.0 / 40.0);
            let mut shore = (0, 0);
            for (one, scaled) in one.iter().zip(heights(scale_heights(original, scale))) {
                let off = if *one >= 62.0 {
                    scaled - (62.0 + (one - 62.0) * scale)
                } else {
                    scaled - (62.0 + (one - 62.0) * depth_scale)
                };
                if (52.0..=72.0).contains(one) {
                    // Rivers, lakes and beaches carve by fixed block depths near the sea, which don't scale.
                    shore = (shore.0 + 1, shore.1 + usize::from(off.abs() <= 2.0));
                    assert!(off.abs() <= 8.0, "scale {scale}: {one} -> {scaled}");
                } else {
                    // Within the rounding of the scaled levels.
                    assert!(off.abs() <= 2.5, "scale {scale}: {one} -> {scaled}");
                }
            }
            assert!(shore.1 * 2 > shore.0, "scale {scale}: {shore:?}");
        }
    }
}
