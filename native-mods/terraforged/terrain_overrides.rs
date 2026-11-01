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

use super::{terraforged::Module, terraforged_terrain_blender::TerrainNoise};
use std::{collections::BTreeMap, sync::Arc};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TerrainGeneralSettings {
    pub terrain_region_size: i32,
    pub region_jitter: f32,
    pub region_blending: f32,
    pub terrain_seed_offset: i32,
    pub global_vertical_scale: f32,
    pub fancy_mountains: bool,
    pub mountain_width: f32,
}

impl Default for TerrainGeneralSettings {
    fn default() -> Self {
        Self {
            terrain_region_size: 800,
            region_jitter: 0.8,
            region_blending: 0.4,
            terrain_seed_offset: 0,
            global_vertical_scale: 1.0,
            fancy_mountains: true,
            mountain_width: 3.5,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TerrainEntrySettings {
    pub weight: f32,
    pub vertical_scale: f32,
    pub horizontal_scale: f32,
    pub base_scale: f32,
}

impl Default for TerrainEntrySettings {
    fn default() -> Self {
        Self {
            weight: 1.0,
            vertical_scale: 1.0,
            horizontal_scale: 1.0,
            base_scale: 1.0,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CurrentTerrainSettings {
    pub general: TerrainGeneralSettings,
    pub terrains: BTreeMap<String, TerrainEntrySettings>,
}

pub fn apply_terrain_overrides(
    terrains: &[Arc<TerrainNoise>],
    ids: &[String],
    settings: &CurrentTerrainSettings,
) -> Vec<Arc<TerrainNoise>> {
    assert_eq!(terrains.len(), ids.len());
    let general = settings.general;
    let global_scale = general.global_vertical_scale != 1.0;
    let mut stretch = general.mountain_width;
    if !(stretch > 0.0) {
        stretch = 1.0;
    }
    if settings.terrains.is_empty() && !global_scale && general.fancy_mountains && stretch == 1.0 {
        return terrains.to_vec();
    }

    let mut result = Vec::with_capacity(terrains.len());
    for (terrain, id) in terrains.iter().zip(ids) {
        if !general.fancy_mountains && id.contains("ridge") {
            continue;
        }
        let entry = settings.terrains.get(id);
        let stretched = stretch != 1.0 && is_mountain(id);
        if entry.is_none() && !global_scale && !stretched {
            result.push(Arc::clone(terrain));
            continue;
        }

        let mut noise = terrain.noise().clone();
        let mut weight = terrain.weight();
        if let Some(entry) = entry {
            weight = entry.weight;
            if entry.horizontal_scale != 1.0 && entry.horizontal_scale > 0.0 {
                let frequency = 1.0 / 0.01_f32.max(entry.horizontal_scale);
                noise = noise.frequency(Module::constant(frequency), Module::constant(frequency));
            }
            if entry.vertical_scale != 1.0 {
                noise = noise.scale(Module::constant(entry.vertical_scale));
            }
            if entry.base_scale != 1.0 {
                noise = noise
                    .bias(Module::constant((entry.base_scale - 1.0) * 0.1))
                    .clamp(Module::constant(0.0), Module::constant(1.0));
            }
        }
        if stretched {
            let frequency = 1.0 / stretch;
            noise = noise.frequency(Module::constant(frequency), Module::constant(frequency));
        }
        if global_scale {
            noise = noise.scale(Module::constant(general.global_vertical_scale));
        }
        result.push(Arc::new(TerrainNoise::new(
            terrain.terrain().clone(),
            weight,
            noise,
        )));
    }
    if result.is_empty() {
        terrains.to_vec()
    } else {
        result
    }
}

fn is_mountain(id: &str) -> bool {
    id.contains("mountain")
        || id.contains("dolomites")
        || id.contains("torridonian")
        || id.contains("dales")
}

#[cfg(test)]
mod tests {
    use super::super::{
        terraforged::{Source, SourceConfig, SourceKind},
        terraforged_engine::TerrainRegistry,
    };
    use super::*;

    fn terrain(id: i32, weight: f32) -> Arc<TerrainNoise> {
        Arc::new(TerrainNoise::new(
            TerrainRegistry::new().get_by_id(id),
            weight,
            Module::source(Source::new(
                SourceKind::Perlin,
                SourceConfig {
                    seed: 91,
                    octaves: 2,
                    frequency: 0.01,
                    ..SourceConfig::default()
                },
            )),
        ))
    }

    #[test]
    fn current_defaults_only_rebuild_mountain_assets() {
        let plains = terrain(8, 1.0);
        let mountains = terrain(12, 2.0);
        let inputs = [Arc::clone(&plains), Arc::clone(&mountains)];
        let ids = ["plains".to_owned(), "mountains_1".to_owned()];
        let result = apply_terrain_overrides(&inputs, &ids, &CurrentTerrainSettings::default());

        assert!(Arc::ptr_eq(&result[0], &plains));
        assert!(!Arc::ptr_eq(&result[1], &mountains));
        assert_eq!(result[1].weight().to_bits(), mountains.weight().to_bits());
        let expected = TerrainNoise::new(
            mountains.terrain().clone(),
            mountains.weight(),
            mountains
                .noise()
                .clone()
                .frequency(Module::constant(1.0 / 3.5), Module::constant(1.0 / 3.5)),
        );
        let actual = result[1]
            .noise()
            .try_value_seed_first(17, 350.0, -175.0)
            .unwrap();
        let expected = expected
            .noise()
            .try_value_seed_first(17, 350.0, -175.0)
            .unwrap();
        assert_eq!(actual.to_bits(), expected.to_bits());
    }

    #[test]
    fn ridge_filter_and_empty_fallback_match_current_source() {
        let ridge = terrain(12, 1.0);
        let settings = CurrentTerrainSettings {
            general: TerrainGeneralSettings {
                fancy_mountains: false,
                mountain_width: 1.0,
                ..TerrainGeneralSettings::default()
            },
            ..CurrentTerrainSettings::default()
        };
        let result = apply_terrain_overrides(
            &[Arc::clone(&ridge)],
            &["mountains_ridge_1".to_owned()],
            &settings,
        );
        assert!(Arc::ptr_eq(&result[0], &ridge));
    }

    #[test]
    fn per_terrain_scales_apply_before_global_and_mountain_scales() {
        let mountain = terrain(12, 0.5);
        let expected = TerrainNoise::new(
            mountain.terrain().clone(),
            3.0,
            mountain
                .noise()
                .clone()
                .frequency(Module::constant(0.25), Module::constant(0.25))
                .scale(Module::constant(0.8))
                .bias(Module::constant(0.05))
                .clamp(Module::constant(0.0), Module::constant(1.0))
                .frequency(Module::constant(0.5), Module::constant(0.5))
                .scale(Module::constant(0.75)),
        );
        let mut settings = CurrentTerrainSettings {
            general: TerrainGeneralSettings {
                global_vertical_scale: 0.75,
                mountain_width: 2.0,
                ..TerrainGeneralSettings::default()
            },
            ..CurrentTerrainSettings::default()
        };
        settings.terrains.insert(
            "mountains_1".to_owned(),
            TerrainEntrySettings {
                weight: 3.0,
                vertical_scale: 0.8,
                horizontal_scale: 4.0,
                base_scale: 1.5,
            },
        );
        let result = apply_terrain_overrides(&[mountain], &["mountains_1".to_owned()], &settings);
        assert_eq!(result[0].weight().to_bits(), 3.0_f32.to_bits());
        for (x, z) in [(0.0, 0.0), (128.0, -64.0), (-511.5, 93.25)] {
            assert_eq!(
                result[0]
                    .noise()
                    .try_value_seed_first(17, x, z)
                    .unwrap()
                    .to_bits(),
                expected
                    .noise()
                    .try_value_seed_first(17, x, z)
                    .unwrap()
                    .to_bits()
            );
        }
    }
}
