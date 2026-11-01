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

use super::terraforged_engine::{ControlPoints, Terrain, TerrainRegistry};

pub use super::terraforged_biome_sampler::{BEACH, COAST, DEEP_OCEAN, INLAND, SHALLOW_OCEAN};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FalloffPoint {
    pub control_point: f32,
    pub min: f32,
    pub max: f32,
}

impl FalloffPoint {
    pub const fn new(control_point: f32, min: f32, max: f32) -> Self {
        Self {
            control_point,
            min,
            max,
        }
    }
}

pub fn terrain_type(continent_noise: f32, terrains: &TerrainRegistry) -> Terrain {
    if continent_noise < SHALLOW_OCEAN {
        terrains.get_by_id(1)
    } else if continent_noise < BEACH {
        terrains.get_by_id(2)
    } else if continent_noise < COAST {
        terrains.get_by_id(3)
    } else {
        terrains.get_by_id(0)
    }
}

pub fn falloff_curve(control_points: ControlPoints) -> [FalloffPoint; 5] {
    [
        FalloffPoint::new(control_points.inland, 1.0, 1.0),
        FalloffPoint::new(control_points.coast, COAST, 1.0),
        FalloffPoint::new(control_points.beach, BEACH, COAST),
        FalloffPoint::new(control_points.shallow_ocean, SHALLOW_OCEAN, BEACH),
        FalloffPoint::new(control_points.deep_ocean, DEEP_OCEAN, SHALLOW_OCEAN),
    ]
}

pub fn falloff(continent_noise: f32, curve: &[FalloffPoint]) -> f32 {
    let mut previous = 1.0;
    for point in curve {
        if continent_noise >= point.control_point {
            return map(
                continent_noise,
                point.control_point,
                previous,
                point.min,
                point.max,
            );
        }
        previous = point.control_point;
    }
    map(continent_noise, 0.0, previous, 0.0, DEEP_OCEAN)
}

fn map(value: f32, minimum: f32, maximum: f32, output_min: f32, output_max: f32) -> f32 {
    let range = maximum - minimum;
    let clamped = if value < minimum {
        minimum
    } else if value > maximum {
        maximum
    } else {
        value
    };
    let delta = clamped - minimum;
    let alpha = if delta > range { 1.0 } else { delta / range };
    output_min + alpha * (output_max - output_min)
}

#[cfg(test)]
mod tests {
    use super::super::terraforged_engine::WorldControlPoints;
    use super::*;

    fn control_points() -> ControlPoints {
        ControlPoints::new(WorldControlPoints {
            deep_ocean: 0.08,
            shallow_ocean: 0.22,
            beach: 0.34,
            coast: 0.47,
            inland: 0.63,
        })
    }

    #[test]
    fn terrain_types_match_java_boundaries() {
        let terrains = TerrainRegistry::new();
        let fixtures = [
            (0x0000_0000, "deep_ocean"),
            (0x3e7f_ffff, "deep_ocean"),
            (0x3e80_0000, "ocean"),
            (0x3eff_ffff, "ocean"),
            (0x3f00_0000, "coast"),
            (0x3f0c_cccc, "coast"),
            (0x3f0c_cccd, "none"),
            (0x7fc0_0000, "none"),
        ];
        for (input, expected) in fixtures {
            assert_eq!(
                terrain_type(f32::from_bits(input), &terrains).name(),
                expected
            );
        }
    }

    #[test]
    fn falloff_curve_matches_java_raw_bits() {
        let actual = falloff_curve(control_points());
        let expected = [
            (0x3f21_47ae, 0x3f80_0000, 0x3f80_0000),
            (0x3ef0_a3d7, 0x3f0c_cccd, 0x3f80_0000),
            (0x3eae_147b, 0x3f00_0000, 0x3f0c_cccd),
            (0x3e61_47ae, 0x3e80_0000, 0x3f00_0000),
            (0x3da3_d70a, 0x3dcc_cccd, 0x3e80_0000),
        ];
        for (actual, expected) in actual.iter().zip(expected) {
            assert_eq!(actual.control_point.to_bits(), expected.0);
            assert_eq!(actual.min.to_bits(), expected.1);
            assert_eq!(actual.max.to_bits(), expected.2);
        }
    }

    #[test]
    fn falloff_matches_java_raw_bits() {
        let curve = falloff_curve(control_points());
        let fixtures = [
            (0xbe80_0000, 0x0000_0000),
            (0x0000_0000, 0x0000_0000),
            (0x3d23_d70a, 0x3d4c_cccd),
            (0x3da3_d70a, 0x3dcc_cccd),
            (0x3e19_999a, 0x3e33_3334),
            (0x3e61_47ae, 0x3e80_0000),
            (0x3e99_999a, 0x3ed5_5556),
            (0x3eae_147b, 0x3f00_0000),
            (0x3ecc_cccd, 0x3f05_e85f),
            (0x3ef0_a3d7, 0x3f0c_cccd),
            (0x3f0c_cccd, 0x3f46_6668),
            (0x3f21_47ae, 0x3f80_0000),
            (0x3f4c_cccd, 0x3f80_0000),
            (0x3f80_0000, 0x3f80_0000),
            (0x3fa0_0000, 0x3f80_0000),
        ];
        for (input, expected) in fixtures {
            assert_eq!(falloff(f32::from_bits(input), &curve).to_bits(), expected);
        }
    }
}
