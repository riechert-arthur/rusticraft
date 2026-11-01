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

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NoiseLevels {
    pub auto: bool,
    pub scale: f32,
    pub unit: f32,
    pub depth_min: f32,
    pub depth_range: f32,
    pub height_min: f32,
    pub base_range: f32,
    pub height_range: f32,
    pub frequency: f32,
}

impl NoiseLevels {
    pub fn new(
        auto_scale: bool,
        scale: f32,
        sea_level: i32,
        sea_floor: i32,
        world_height: i32,
        base_height: i32,
    ) -> Self {
        let unit = 1.0 / world_height as f32;
        let depth_min = sea_floor as f32 / world_height as f32;
        let height_min = sea_level as f32 / world_height as f32;
        let base_range = base_height as f32 / world_height as f32;
        Self {
            auto: auto_scale,
            scale,
            unit,
            depth_min,
            depth_range: height_min - depth_min,
            height_min,
            base_range,
            height_range: 1.0 - (height_min + base_range),
            frequency: Self::calc_frequency(world_height - sea_level, auto_scale, scale),
        }
    }

    pub fn scale(self, level: i32) -> f32 {
        level as f32 * self.unit
    }

    pub fn to_depth_noise(self, noise: f32) -> f32 {
        self.depth_min + noise * self.depth_range
    }

    pub fn to_height_noise(self, base_noise: f32, height_noise: f32) -> f32 {
        self.height_min + self.base_range * base_noise + self.height_range * height_noise
    }

    pub fn floor(self, value: f32) -> f32 {
        super::terraforged::floor(value / self.unit) as f32 * self.unit
    }

    pub fn ceil(self, value: f32) -> f32 {
        self.unit + super::terraforged::floor(value / self.unit) as f32 * self.unit
    }

    pub fn calc_frequency(vertical_range: i32, auto: bool, scale: f32) -> f32 {
        let scale = if scale <= 0.0 { 1.0 } else { scale };
        if auto {
            (TerrainLevels::LEGACY_GEN_DEPTH - TerrainLevels::DEFAULT_SEA_LEVEL) as f32
                / vertical_range as f32
                * scale
        } else {
            scale
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TerrainLevels {
    pub min_y: i32,
    pub max_y: i32,
    pub base_height: i32,
    pub sea_floor: i32,
    pub sea_level: i32,
    pub noise_levels: NoiseLevels,
}

impl TerrainLevels {
    pub const MIN_MIN_Y: i32 = -2_032;
    pub const MAX_MIN_Y: i32 = 0;
    pub const MIN_SEA_LEVEL: i32 = 32;
    pub const MAX_SEA_LEVEL: i32 = 4_064;
    pub const MIN_SEA_FLOOR: i32 = 0;
    pub const MAX_SEA_FLOOR: i32 = Self::MAX_SEA_LEVEL;
    pub const MIN_MAX_Y: i32 = 128;
    pub const MAX_MAX_Y: i32 = 4_064;
    pub const DEFAULT_SCALE: f32 = 1.0;
    pub const DEFAULT_MIN_Y: i32 = -64;
    pub const DEFAULT_MAX_Y: i32 = 480;
    pub const DEFAULT_BASE_HEIGHT: i32 = 128;
    pub const DEFAULT_SEA_LEVEL: i32 = 62;
    pub const DEFAULT_SEA_FLOOR: i32 = Self::DEFAULT_SEA_LEVEL - 40;
    pub const LEGACY_GEN_DEPTH: i32 = 256;

    pub fn new(
        auto_scale: bool,
        scale: f32,
        min_y: i32,
        max_y: i32,
        base_height: i32,
        sea_level: i32,
        sea_floor: i32,
    ) -> Self {
        fn clamp(value: i32, min: i32, max: i32) -> i32 {
            if value < min {
                min
            } else if value > max {
                max
            } else {
                value
            }
        }
        let min_y = clamp(min_y, Self::MIN_MIN_Y, Self::MAX_MIN_Y);
        let sea_level = clamp(sea_level, Self::MIN_SEA_LEVEL, max_y >> 1);
        let max_y = clamp(max_y, Self::MIN_MAX_Y, Self::MAX_MAX_Y);
        let sea_floor = clamp(sea_floor, min_y, sea_level - 1);
        let base_height = clamp(base_height, sea_level, max_y);
        Self {
            min_y,
            max_y,
            base_height,
            sea_floor,
            sea_level,
            noise_levels: NoiseLevels::new(
                auto_scale,
                scale,
                sea_level,
                sea_floor,
                max_y,
                base_height,
            ),
        }
    }

    pub fn unconfigured() -> Self {
        Self {
            min_y: -Self::DEFAULT_MIN_Y,
            max_y: Self::DEFAULT_MAX_Y,
            base_height: Self::DEFAULT_BASE_HEIGHT,
            sea_floor: Self::DEFAULT_SEA_FLOOR,
            sea_level: Self::DEFAULT_SEA_LEVEL,
            noise_levels: NoiseLevels::new(
                false,
                Self::DEFAULT_SCALE,
                Self::DEFAULT_SEA_LEVEL,
                Self::DEFAULT_SEA_FLOOR,
                Self::DEFAULT_MAX_Y,
                Self::DEFAULT_BASE_HEIGHT,
            ),
        }
    }

    pub fn scaled_height(self, height_noise: f32) -> f32 {
        height_noise * self.max_y as f32
    }

    pub fn scaled_base_level(self, water_level_noise: f32) -> f32 {
        self.noise_levels.to_height_noise(water_level_noise, 0.0) * self.max_y as f32
    }

    pub fn height(self, scaled_height: f32) -> i32 {
        super::terraforged::floor(scaled_height)
    }
}

impl Default for TerrainLevels {
    fn default() -> Self {
        Self::new(
            true,
            Self::DEFAULT_SCALE,
            Self::DEFAULT_MIN_Y,
            Self::DEFAULT_MAX_Y,
            Self::DEFAULT_BASE_HEIGHT,
            Self::DEFAULT_SEA_LEVEL,
            Self::DEFAULT_SEA_FLOOR,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terrain_levels_match_terraforged_source() {
        let cases = [
            (
                TerrainLevels::default(),
                (-64, 480, 128, 62, 22),
                [
                    0x3b088889, 0x3d3bbbbc, 0x3e044444, 0x3e888889, 0x3f1aaaaa, 0x3daaaaaa,
                    0x3eeda076,
                ],
            ),
            (
                TerrainLevels::new(false, 0.0, -9_999, 64, 1, 9_999, -9_999),
                (-2_032, 128, 32, 32, -2_032),
                [
                    0x3c000000, 0xc17e0000, 0x3e800000, 0x3e800000, 0x3f000000, 0x41810000,
                    0x3f800000,
                ],
            ),
            (
                TerrainLevels::new(true, 2.5, -32, 320, 100, 80, 20),
                (-32, 320, 100, 80, 20),
                [
                    0x3b4ccccd, 0x3d800000, 0x3e800000, 0x3ea00000, 0x3ee00000, 0x3e400000,
                    0x40015555,
                ],
            ),
            (
                TerrainLevels::new(false, 1.0, -64, 9_999, 9_999, 9_999, 22),
                (-64, 4_064, 4_064, 4_999, 22),
                [
                    0x39810204, 0x3bb162c6, 0x3f9d72e6, 0x3f800000, 0xbf9d72e6, 0x3f9cc183,
                    0x3f800000,
                ],
            ),
        ];
        for (levels, expected, bits) in cases {
            assert_eq!(
                (
                    levels.min_y,
                    levels.max_y,
                    levels.base_height,
                    levels.sea_level,
                    levels.sea_floor,
                ),
                expected
            );
            let noise = levels.noise_levels;
            assert_eq!(
                [
                    noise.unit.to_bits(),
                    noise.depth_min.to_bits(),
                    noise.height_min.to_bits(),
                    noise.base_range.to_bits(),
                    noise.height_range.to_bits(),
                    noise.depth_range.to_bits(),
                    noise.frequency.to_bits(),
                ],
                bits
            );
        }
        assert_eq!(TerrainLevels::unconfigured().min_y, 64);
    }

    #[test]
    fn constructor_bounds_match_current_java_oracle() {
        let cases = [
            (
                [0, 0, 128, 62, 0],
                [0, 128, 128, 0, -1],
                [
                    0x3c000000, 0xbc000000, 0, 0x3f800000, 0, 0x3c000000, 0x3f800000,
                ],
            ),
            (
                [0, 0, 128, 0, 0],
                [0, 128, 128, 32, 0],
                [
                    0x3c000000, 0, 0x3e800000, 0x3f800000, 0xbe800000, 0x3e800000, 0x3f800000,
                ],
            ),
            (
                [-64, 10_000, 6_000, 5_000, 22],
                [-64, 4_064, 4_064, 5_000, 22],
                [
                    0x39810204, 0x3bb162c6, 0x3f9d7af6, 0x3f800000, 0xbf9d7af6, 0x3f9cc993,
                    0x3f800000,
                ],
            ),
            (
                [-9_999, i32::MIN, 128, 62, 0],
                [-2_032, 128, 128, -1_073_741_824, -1_073_741_825],
                [
                    0x3c000000, 0xcb000000, 0xcb000000, 0x3f800000, 0x4b000000, 0, 0x3f800000,
                ],
            ),
            (
                [-64, 128, 128, 62, 22],
                [-64, 128, 128, 62, 22],
                [
                    0x3c000000, 0x3e300000, 0x3ef80000, 0x3f800000, 0xbef80000, 0x3ea00000,
                    0x3f800000,
                ],
            ),
            (
                [-64, 4_064, 128, 62, 22],
                [-64, 4_064, 128, 62, 22],
                [
                    0x39810204, 0x3bb162c6, 0x3c79f3e8, 0x3d010204, 0x3f740810, 0x3c214285,
                    0x3f800000,
                ],
            ),
        ];
        for ([min, max, base, sea, floor], expected, bits) in cases {
            let levels = TerrainLevels::new(false, 0.0, min, max, base, sea, floor);
            assert_eq!(
                [
                    levels.min_y,
                    levels.max_y,
                    levels.base_height,
                    levels.sea_level,
                    levels.sea_floor
                ],
                expected
            );
            let noise = levels.noise_levels;
            assert_eq!(
                [
                    noise.unit,
                    noise.depth_min,
                    noise.height_min,
                    noise.base_range,
                    noise.height_range,
                    noise.depth_range,
                    noise.frequency
                ]
                .map(f32::to_bits),
                bits
            );
        }
    }

    #[test]
    fn noise_level_transforms_match_terraforged_source() {
        let levels = TerrainLevels::default();
        let noise = levels.noise_levels;
        assert_eq!(noise.to_depth_noise(-1.0).to_bits(), 0xbd199998);
        assert_eq!(noise.to_depth_noise(0.25).to_bits(), 0x3d888888);
        assert_eq!(noise.to_height_noise(0.25, 0.25).to_bits(), 0x3eb19999);
        assert_eq!(levels.scaled_height(0.25).to_bits(), 0x42f00000);
        assert_eq!(levels.height(levels.scaled_height(-1.0)), -481);
        assert_eq!(noise.floor(0.25).to_bits(), 0x3e7ddddf);
        assert_eq!(noise.ceil(0.25).to_bits(), 0x3e800001);
    }

    #[test]
    fn height_rounding_matches_noise_util_oracle() {
        let levels = TerrainLevels::default();
        for (value, expected) in [
            (-480.0, -481),
            (-1.0, -2),
            (-1.25, -2),
            (-0.0, 0),
            (0.0, 0),
            (1.0, 1),
            (f32::NAN, -1),
            (f32::NEG_INFINITY, i32::MAX),
            (f32::INFINITY, i32::MAX),
        ] {
            assert_eq!(levels.height(value), expected, "{value}");
        }
        let noise = NoiseLevels::new(false, 1.0, 62, 22, 256, 128);
        assert_eq!(noise.floor(-0.25).to_bits(), 0xbe820000);
        assert_eq!(noise.ceil(-0.25).to_bits(), 0xbe800000);
    }
}
