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
    terraforged::java_min,
    terraforged_continent_config::{ContinentConfig, RiverConfig},
    terraforged_continent_points::{BEACH, COAST},
    terraforged_river_nodes::{CarverSample, NodeSample, clamp, map},
    terraforged_terrain_data::NoiseSample,
    terraforged_terrain_levels::NoiseLevels,
};

pub struct RiverCarver {
    levels: NoiseLevels,
    river_config: RiverConfig,
    lake_config: RiverConfig,
    pub blend_radius: f32,
}

impl RiverCarver {
    pub fn new(levels: NoiseLevels, config: ContinentConfig) -> Self {
        let frequency = levels.frequency * (1.0 / config.shape.scale as f32);
        let mut river_config = config.rivers.rivers;
        let mut lake_config = config.rivers.lakes;
        river_config.scale(frequency);
        lake_config.scale(frequency);
        let valley_width = -java_min(
            -river_config.valley_width.max,
            -lake_config.valley_width.max,
        );
        Self {
            levels,
            river_config,
            lake_config,
            blend_radius: java_min(1.0, valley_width + 0.2),
        }
    }

    pub fn carve_with_erosion(
        &self,
        erosion: f32,
        sample: &mut NoiseSample,
        carver: &CarverSample,
    ) {
        let modifier = map(sample.continent_noise, COAST, 1.0, 1.0 - COAST);
        let base = sample.base_noise * modifier;
        let base = self.carve_node(
            sample,
            &carver.river,
            self.river_config,
            base,
            modifier,
            erosion,
        );
        let base = self.carve_node(
            sample,
            &carver.lake,
            self.lake_config,
            base,
            modifier,
            erosion,
        );
        sample.base_noise = base;
        if sample.continent_noise < BEACH {
            sample.river_noise = 1.0;
        }
    }

    fn carve_node(
        &self,
        sample: &mut NoiseSample,
        node: &NodeSample,
        config: RiverConfig,
        base: f32,
        modifier: f32,
        erosion: f32,
    ) -> f32 {
        let modified_base = base_noise(sample, node, config, modifier);
        if modified_base == -1.0 {
            return base;
        }
        let base_level = self.levels.to_height_noise(modified_base, 0.0);
        self.carve_height(base_level, erosion, sample, node, config);
        modified_base
    }

    fn carve_height(
        &self,
        base_level: f32,
        erosion: f32,
        sample: &mut NoiseSample,
        node: &NodeSample,
        config: RiverConfig,
    ) {
        if node.is_invalid() {
            return;
        }
        let mut height = sample.height_noise;
        let position = node.position;
        let distance = node.distance;
        let valley_width = config.valley_width.at(position);
        let bank_width = config.bank_width.at(position);
        let bank_depth = config.bank_depth.at(position);
        let bed_width = config.bed_width.at(position);
        let bed_depth = config.bed_depth.at(position);
        let bed_level = base_level - bed_depth * self.levels.unit;
        let bank_level = base_level + bank_depth * self.levels.unit;
        let valley_alpha = alpha(distance, bank_width, valley_width);
        let shape_alpha = alpha(sample.base_noise, 0.4, 0.6);
        let valley_alpha = lerp(valley_alpha * valley_alpha, valley_alpha, shape_alpha);
        if valley_alpha < 1.0 {
            let level = java_min(bank_level, height);
            let fade = 1.0 - map(valley_alpha, 0.975, 1.0, 0.025);
            let modifier = 1.0 - ((erosion * config.erosion) * fade);
            height = lerp(level, height, valley_alpha * modifier);
            sample.river_noise *= clamp(
                0.05 + alpha(distance, bank_width, valley_width) / 0.95,
                0.0,
                1.0,
            );
        }
        let river_alpha = alpha(distance, bed_width, bank_width);
        if river_alpha < 1.0 {
            let level = java_min(bed_level, height);
            height = lerp(level, height, river_alpha);
            sample.terrain_type = node.terrain.clone();
            sample.river_noise *= clamp(alpha(height, base_level, bank_level), 0.0, 1.0);
        }
        sample.height_noise = height;
    }
}

fn base_noise(sample: &NoiseSample, node: &NodeSample, config: RiverConfig, modifier: f32) -> f32 {
    if node.is_invalid() {
        return -1.0;
    }
    let valley_radius = config.valley_width.at(node.position);
    if node.distance >= valley_radius {
        return -1.0;
    }
    let bank_radius = config.bank_width.at(node.position);
    if node.distance <= bank_radius {
        return node.level * modifier;
    }
    let alpha = (node.distance - bank_radius) / (valley_radius - bank_radius);
    lerp(node.level, sample.base_noise, alpha) * modifier
}

fn alpha(value: f32, minimum: f32, maximum: f32) -> f32 {
    if value <= minimum {
        0.0
    } else if value >= maximum {
        1.0
    } else {
        (value - minimum) / (maximum - minimum)
    }
}

fn lerp(minimum: f32, maximum: f32, alpha: f32) -> f32 {
    minimum + alpha * (maximum - minimum)
}

#[cfg(test)]
mod tests {
    use super::super::terraforged_engine::TerrainRegistry;
    use super::*;

    #[test]
    fn carving_matches_current_java_with_fixed_erosion() {
        let carver = RiverCarver::new(
            NoiseLevels::new(false, 1.0, 62, 22, 256, 128),
            ContinentConfig::default(),
        );
        let expected = [
            (0x3f016c16, 0x3f59999a, 0x3f800000, "none"),
            (0x3e9f49f4, 0x3ec4a4fa, 0, "river"),
            (0x3e9f49f4, 0x3eca861e, 0, "river"),
            (0x3ebde00f, 0x3f0e744c, 0x3ebf2e24, "none"),
            (0x3f016c16, 0x3f59999a, 0x3f800000, "none"),
            (0, 0x3e75c248, 0x3f800000, "river"),
            (0x3edb05b0, 0x3eca861e, 0, "lake"),
        ];
        for (index, distance) in [f32::NAN, 0.0, 0.03, 0.15, 1.0, 0.03, 0.03]
            .into_iter()
            .enumerate()
        {
            let mut sample = NoiseSample {
                continent_noise: if index == 5 { 0.1 } else { 0.9 },
                base_noise: 0.65,
                height_noise: 0.85,
                ..NoiseSample::default()
            };
            let mut nodes = CarverSample::new(&TerrainRegistry::default());
            nodes.river.position = 0.6;
            nodes.river.level = 0.4;
            nodes.river.distance = distance;
            if index == 6 {
                nodes.lake.distance = 0.01;
                nodes.lake.position = 0.8;
                nodes.lake.level = 0.55;
            }
            carver.carve_with_erosion(0.75, &mut sample, &nodes);
            assert_eq!(
                (
                    sample.base_noise.to_bits(),
                    sample.height_noise.to_bits(),
                    sample.river_noise.to_bits(),
                    sample.terrain_type.name()
                ),
                expected[index]
            );
        }
        assert_eq!(carver.blend_radius, 0.7);
    }

    #[test]
    fn interpolation_preserves_nan_and_boundary_behavior() {
        assert_eq!(alpha(0.0, 0.0, 0.0), 0.0);
        assert_eq!(alpha(1.0, 0.0, 0.0), 1.0);
        assert!(alpha(f32::NAN, 0.0, 1.0).is_nan());
        assert!(java_min(0.0, f32::NAN).is_nan());
        assert_eq!(java_min(0.0, -0.0).to_bits(), (-0.0f32).to_bits());
    }
}
