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
    terraforged::floor, terraforged_engine::TerrainProperties,
    terraforged_terrain_data::TerrainData,
};

/// TerraForged's `MathUtil.clamp(int, int, int)`: `value < min ? min : value > max ? max : value`. Unlike
/// `i32::clamp` it never panics when `min > max` (then `min` wins for values below it and `max` for the rest).
fn java_clamp(value: i32, min: i32, max: i32) -> i32 {
    if value < min {
        min
    } else if value > max {
        max
    } else {
        value
    }
}

/// TerraForged's `NoiseLevels` (`com.terraforged.mod.worldgen.noise`): the world's sea floor, sea level and base
/// height as fractions of the build height (`max_y`, counted from block 0), and the horizontal frequency.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NoiseLevels {
    pub auto: bool,
    /// The configured horizontal scale, as given (`calc_frequency` replaces `<= 0` by 1 only for `frequency`).
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
    /// Java's constructor, minus its debug log line.
    pub fn new(
        auto_scale: bool,
        scale: f32,
        sea_level: i32,
        sea_floor: i32,
        world_height: i32,
        base_height: i32,
    ) -> Self {
        let height_min = sea_level as f32 / world_height as f32;
        let depth_min = sea_floor as f32 / world_height as f32;
        let base_range = base_height as f32 / world_height as f32;
        Self {
            auto: auto_scale,
            scale,
            unit: 1.0 / world_height as f32,
            depth_min,
            depth_range: height_min - depth_min,
            height_min,
            base_range,
            height_range: 1.0 - (height_min + base_range),
            frequency: Self::calc_frequency(
                world_height.wrapping_sub(sea_level),
                auto_scale,
                scale,
            ),
        }
    }

    /// `NoiseLevels.getDefault()`: the default terrain levels' noise levels.
    pub fn get_default() -> Self {
        TerrainLevels::default().noise_levels
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

    /// `NoiseUtil.floor(value / unit) * unit`: the `int` floor (`NoiseUtil.floor`, which is one lower for negative
    /// whole numbers, saturates and gives -1 for NaN), back to `float`.
    pub fn floor(self, value: f32) -> f32 {
        floor(value / self.unit) as f32 * self.unit
    }

    pub fn ceil(self, value: f32) -> f32 {
        self.unit + floor(value / self.unit) as f32 * self.unit
    }

    pub fn calc_frequency(vertical_range: i32, auto: bool, scale: f32) -> f32 {
        let scale = if scale <= 0.0 { 1.0 } else { scale };
        if !auto {
            return scale;
        }
        let frequency = (TerrainLevels::LEGACY_GEN_DEPTH - TerrainLevels::DEFAULT_SEA_LEVEL) as f32
            / vertical_range as f32;
        frequency * scale
    }
}

/// TerraForged's `TerrainLevels` (`com.terraforged.mod.worldgen.terrain`): the world's build bounds in block Y.
/// `max_y` is the exclusive top block and the "world height" every noise fraction divides by, counted from block 0
/// (`min_y` only sets how deep the sea floor may go); `sea_level` is the highest water block (62 by default: water
/// up to Y 62, as in vanilla 26.3's sea level 63). These are separate from Engine's `Levels` (heightmap scale),
/// which TerraForged's terrains build themselves (`ModTerrains`' `new Levels(63, 255)`).
///
/// The limits are TerraForged's, read from 1.18.2's `DimensionType` (`MIN_Y` -2032, `Y_SIZE` 4064: the same in
/// 26.3). The defaults are TerraForged's, not vanilla's: `max_y` 480 (vanilla 26.3's overworld ends at 320) and sea
/// level 62. Not ported: the settings codec (and its field ranges).
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

    /// Java's constructor. As in Java, the sea level's upper bound is half the `max_y` *argument*, before
    /// `max_y` is clamped (so a `max_y` below 64 gives a sea level below 32, and a negative one a negative sea
    /// level), and a bound pair with `min > max` follows `MathUtil.clamp`.
    pub fn new(
        auto_scale: bool,
        scale: f32,
        min_y: i32,
        max_y: i32,
        base_height: i32,
        sea_level: i32,
        sea_floor: i32,
    ) -> Self {
        let clamped_min_y = java_clamp(min_y, Self::MIN_MIN_Y, Self::MAX_MIN_Y);
        let clamped_max_y = java_clamp(max_y, Self::MIN_MAX_Y, Self::MAX_MAX_Y);
        let sea_level = java_clamp(sea_level, Self::MIN_SEA_LEVEL, max_y >> 1);
        let sea_floor = java_clamp(sea_floor, clamped_min_y, sea_level.wrapping_sub(1));
        let base_height = java_clamp(base_height, sea_level, clamped_max_y);
        Self {
            min_y: clamped_min_y,
            max_y: clamped_max_y,
            base_height,
            sea_floor,
            sea_level,
            noise_levels: NoiseLevels::new(
                auto_scale,
                scale,
                sea_level,
                sea_floor,
                clamped_max_y,
                base_height,
            ),
        }
    }

    /// Java's no-argument constructor: unclamped, no auto scale, and `min_y = -Defaults.MIN_Y` (64, Java's sign).
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

    /// Java's `copy()`: rebuilt through the constructor, so it clamps again (`unconfigured().copy()` has
    /// `min_y` 0, and a sea level above `max_y / 2` drops). A plain Rust copy keeps every field instead.
    pub fn copy(self) -> Self {
        Self::new(
            self.noise_levels.auto,
            self.noise_levels.scale,
            self.min_y,
            self.max_y,
            self.base_height,
            self.sea_level,
            self.sea_floor,
        )
    }

    pub fn scaled_height(self, height_noise: f32) -> f32 {
        height_noise * self.max_y as f32
    }

    pub fn scaled_base_level(self, water_level_noise: f32) -> f32 {
        self.noise_levels.to_height_noise(water_level_noise, 0.0) * self.max_y as f32
    }

    /// `getHeight`: `NoiseUtil.floor` (one lower for negative whole numbers: -480.0 gives -481; NaN gives -1).
    pub fn height(self, scaled_height: f32) -> i32 {
        floor(scaled_height)
    }

    /// Java's static `getWaterLevel(x, z, seaLevel, terrainData)`: a river or lake cell whose river noise is
    /// exactly 0 (either sign; NaN is not) fills to its base height, every other cell to `sea_level`. `x`/`z`
    /// wrap into the chunk (`Index.CHUNK` masks them). Panics where Java throws a NullPointerException: a
    /// cell with no terrain.
    pub fn water_level(x: i32, z: i32, sea_level: i32, terrain_data: &TerrainData) -> i32 {
        let river = terrain_data.river().get(x, z);
        let terrain = terrain_data
            .terrain()
            .get(x, z)
            .expect("TerrainLevels.getWaterLevel: null terrain");
        if (terrain.is_river() || terrain.is_lake()) && river == 0.0 {
            terrain_data.base_height_at(x, z)
        } else {
            sea_level
        }
    }
}

/// `TerrainLevels.DEFAULT`: auto scale, TerraForged's defaults.
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

    /// `Float.floatToIntBits`: every NaN prints as 7fc00000.
    fn h(v: f32) -> String {
        format!("{:08x}", if v.is_nan() { 0x7fc0_0000 } else { v.to_bits() })
    }

    fn fields(l: TerrainLevels) -> String {
        let n = l.noise_levels;
        format!(
            "{} {} {} {} {} {} {} {} {} {} {} {} {} {}",
            l.min_y,
            l.max_y,
            l.base_height,
            l.sea_level,
            l.sea_floor,
            n.auto,
            h(n.scale),
            h(n.unit),
            h(n.depth_min),
            h(n.depth_range),
            h(n.height_min),
            h(n.base_range),
            h(n.height_range),
            h(n.frequency)
        )
    }

    const VALUES: [f32; 20] = [
        0.0,
        -0.0,
        1.0,
        -1.0,
        0.25,
        -0.25,
        0.5,
        0.999999,
        1.5,
        -480.0,
        62.5,
        1e-3,
        f32::from_bits(1),
        -f32::from_bits(1),
        3e9,
        -3e9,
        f32::MAX,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NAN,
    ];
    const INTS: [i32; 11] = [0, 1, -1, 62, 63, -64, 320, 480, 4064, i32::MAX, i32::MIN];

    fn transforms(out: &mut String, name: &str, l: TerrainLevels) {
        let n = l.noise_levels;
        out.push_str(&format!("SCALE {name}"));
        for i in INTS {
            out.push_str(&format!(" {}", h(n.scale(i))));
        }
        out.push('\n');
        for v in VALUES {
            out.push_str(&format!(
                "X {name} {} {} {} {} {} {} {} {} {} {} {}\n",
                h(v),
                h(n.to_depth_noise(v)),
                h(n.to_height_noise(v, 0.0)),
                h(n.to_height_noise(0.25, v)),
                h(n.to_height_noise(v, v)),
                h(n.floor(v)),
                h(n.ceil(v)),
                h(l.scaled_height(v)),
                h(l.scaled_base_level(v)),
                l.height(v),
                l.height(l.scaled_height(v))
            ));
        }
    }

    /// TerraForged 1.18.2-0.3.1's own `TerrainLevels`/`NoiseLevels` bytecode, from `TerrainLevelsOracle.java`
    /// (`.local/terraforged/oracles/terrain_levels/`, stubbing only `DimensionType` and the logger): the default,
    /// no-argument and constructed levels (clamps with `min > max`, sea levels from a small, negative or huge
    /// `max_y` argument, NaN, infinite, negative-zero and subnormal scales) and their `copy()`, `calcFrequency`
    /// (zero and negative ranges) and every transform over signed zeros, subnormals, huge values, infinities and
    /// NaN.
    #[test]
    fn terrain_levels_match_terraforged() {
        const EXPECTED: &str = "\
DEFAULT -64 480 128 62 22 true 3f800000 3b088889 3d3bbbbc 3daaaaaa 3e044444 3e888889 3f1aaaaa 3eeda076
COPY -64 480 128 62 22 true 3f800000 3b088889 3d3bbbbc 3daaaaaa 3e044444 3e888889 3f1aaaaa 3eeda076
NOARG 64 480 128 62 22 false 3f800000 3b088889 3d3bbbbc 3daaaaaa 3e044444 3e888889 3f1aaaaa 3f800000
NOARGCOPY 0 480 128 62 22 false 3f800000 3b088889 3d3bbbbc 3daaaaaa 3e044444 3e888889 3f1aaaaa 3f800000
NEW 0 -2032 128 32 32 -2032 false 00000000 3c000000 c17e0000 41810000 3e800000 3e800000 3f000000 3f800000
NEWCOPY 0 -2032 128 32 32 -2032 false 00000000 3c000000 c17e0000 41810000 3e800000 3e800000 3f000000 3f800000
NEW 1 -32 320 100 80 20 true 40200000 3b4ccccd 3d800000 3e400000 3e800000 3ea00000 3ee00000 40015555
NEWCOPY 1 -32 320 100 80 20 true 40200000 3b4ccccd 3d800000 3e400000 3e800000 3ea00000 3ee00000 40015555
NEW 2 -64 384 128 63 23 true 3f800000 3b2aaaab 3d755555 3dd55556 3e280000 3eaaaaab 3f00aaaa 3f1ab76d
NEWCOPY 2 -64 384 128 63 23 true 3f800000 3b2aaaab 3d755555 3dd55556 3e280000 3eaaaaab 3f00aaaa 3f1ab76d
NEW 3 -64 320 128 63 23 true 3f800000 3b4ccccd 3d933333 3e000000 3e49999a 3ecccccd 3ece6666 3f413ec1
NEWCOPY 3 -64 320 128 63 23 true 3f800000 3b4ccccd 3d933333 3e000000 3e49999a 3ecccccd 3ece6666 3f413ec1
NEW 4 0 128 32 32 0 false 3ebd70a4 3c000000 00000000 3e800000 3e800000 3e800000 3f000000 3ebd70a4
NEWCOPY 4 0 128 32 32 0 false 3ebd70a4 3c000000 00000000 3e800000 3e800000 3e800000 3f000000 3ebd70a4
NEW 5 0 4064 4064 2500 2499 true c0400000 39810204 3f1d6ad6 39810000 3f1d7af6 3f800000 bf1d7af6 3dfe092b
NEWCOPY 5 0 4064 4064 2032 2031 true c0400000 39810204 3effdfbf 39810400 3f000000 3f800000 bf000000 3dc3870e
NEW 6 -2032 4064 4064 2032 -2032 true 3f800000 39810204 bf000000 3f800000 3f000000 3f800000 bf000000 3dc3870e
NEWCOPY 6 -2032 4064 4064 2032 -2032 true 3f800000 39810204 bf000000 3f800000 3f000000 3f800000 bf000000 3dc3870e
NEW 7 -64 128 64 5 0 true 3f800000 3c000000 00000000 3d200000 3d200000 3f000000 3eec0000 3fc9e2dd
NEWCOPY 7 -64 128 64 32 0 true 3f800000 3c000000 00000000 3e800000 3e800000 3f000000 3e800000 40015555
NEW 8 -64 128 64 32 0 true 3f800000 3c000000 00000000 3e800000 3e800000 3f000000 3e800000 40015555
NEWCOPY 8 -64 128 64 32 0 true 3f800000 3c000000 00000000 3e800000 3e800000 3f000000 3e800000 40015555
NEW 9 -64 128 64 -50 -51 true 3f800000 3c000000 becc0000 3c000000 bec80000 3f000000 3f640000 3f8b8170
NEWCOPY 9 -64 128 64 32 -51 true 3f800000 3c000000 becc0000 3f260000 3e800000 3f000000 3e800000 40015555
NEW 10 -64 128 64 32 0 true 3f800000 3c000000 00000000 3e800000 3e800000 3f000000 3e800000 40015555
NEWCOPY 10 -64 128 64 32 0 true 3f800000 3c000000 00000000 3e800000 3e800000 3f000000 3e800000 40015555
NEW 11 -64 4064 40000 40000 0 true 3f800000 39810204 00000000 411d7af6 411d7af6 411d7af6 c1957af6 bbb0e5c9
NEWCOPY 11 -64 4064 4064 2032 0 true 3f800000 39810204 00000000 3f000000 3f000000 3f800000 bf000000 3dc3870e
NEW 12 -64 4064 1073741823 1073741823 1073741822 true 3f800000 39810204 48810204 00000000 48810204 48810204 c90101f4 b4420031
NEWCOPY 12 -64 4064 4064 2032 2031 true 3f800000 39810204 3effdfbf 39810400 3f000000 3f800000 bf000000 3dc3870e
NEW 13 -2032 128 32 32 -2032 true 3f800000 3c000000 c17e0000 41810000 3e800000 3e800000 3f000000 40015555
NEWCOPY 13 -2032 128 32 32 -2032 true 3f800000 3c000000 c17e0000 41810000 3e800000 3e800000 3f000000 40015555
NEW 14 -64 480 128 62 22 true 7fc00000 3b088889 3d3bbbbc 3daaaaaa 3e044444 3e888889 3f1aaaaa 7fc00000
NEWCOPY 14 -64 480 128 62 22 true 7fc00000 3b088889 3d3bbbbc 3daaaaaa 3e044444 3e888889 3f1aaaaa 7fc00000
NEW 15 -64 480 128 62 22 true 7f800000 3b088889 3d3bbbbc 3daaaaaa 3e044444 3e888889 3f1aaaaa 7f800000
NEWCOPY 15 -64 480 128 62 22 true 7f800000 3b088889 3d3bbbbc 3daaaaaa 3e044444 3e888889 3f1aaaaa 7f800000
NEW 16 -64 480 128 62 22 true 80000000 3b088889 3d3bbbbc 3daaaaaa 3e044444 3e888889 3f1aaaaa 3eeda076
NEWCOPY 16 -64 480 128 62 22 true 80000000 3b088889 3d3bbbbc 3daaaaaa 3e044444 3e888889 3f1aaaaa 3eeda076
NEW 17 -64 480 128 62 61 false 00000001 3b088889 3e022222 3b088880 3e044444 3e888889 3f1aaaaa 00000001
NEWCOPY 17 -64 480 128 62 61 false 00000001 3b088889 3e022222 3b088880 3e044444 3e888889 3f1aaaaa 00000001
NEW 18 -64 480 128 62 61 true 3f800000 3b088889 3e022222 3b088880 3e044444 3e888889 3f1aaaaa 3eeda076
NEWCOPY 18 -64 480 128 62 61 true 3f800000 3b088889 3e022222 3b088880 3e044444 3e888889 3f1aaaaa 3eeda076
FREQ 418 3eeda076 3f800000 3f94844a 40200000 3eeda076 3f800000 3eeda076 3f800000 3eeda076 3f800000 7fc00000 7fc00000 7f800000 7f800000 00000000 00000001
FREQ 194 3f800000 3f800000 40200000 40200000 3f800000 3f800000 3f800000 3f800000 3f800000 3f800000 7fc00000 7fc00000 7f800000 7f800000 00000001 00000001
FREQ 1 43420000 3f800000 43f28000 40200000 43420000 3f800000 43420000 3f800000 43420000 3f800000 7fc00000 7fc00000 7f800000 7f800000 000000c2 00000001
FREQ 0 7f800000 3f800000 7f800000 40200000 7f800000 3f800000 7f800000 3f800000 7f800000 3f800000 7fc00000 7fc00000 7f800000 7f800000 7f800000 00000001
FREQ -1 c3420000 3f800000 c3f28000 40200000 c3420000 3f800000 c3420000 3f800000 c3420000 3f800000 7fc00000 7fc00000 ff800000 7f800000 800000c2 00000001
FREQ -400 bef851ec 3f800000 bf9b3334 40200000 bef851ec 3f800000 bef851ec 3f800000 bef851ec 3f800000 7fc00000 7fc00000 ff800000 7f800000 80000000 00000001
FREQ 2147483647 33c20000 3f800000 34728000 40200000 33c20000 3f800000 33c20000 3f800000 33c20000 3f800000 7fc00000 7fc00000 7f800000 7f800000 00000000 00000001
FREQ -2147483648 b3c20000 3f800000 b4728000 40200000 b3c20000 3f800000 b3c20000 3f800000 b3c20000 3f800000 7fc00000 7fc00000 ff800000 7f800000 80000000 00000001
SCALE default 00000000 3b088889 bb088889 3e044445 3e066667 be088889 3f2aaaab 3f800000 41077778 4a888889 ca888889
X default 00000000 3d3bbbbc 3e044444 3e488888 3e044444 00000000 3b088889 00000000 42780000 0 0
X default 80000000 3d3bbbbc 3e044444 3e488888 3e044444 00000000 3b088889 80000000 42780000 0 0
X default 3f800000 3e044444 3ecaaaab 3f4ccccc 3f800000 3f7f7778 3f800000 43f00000 433e0000 1 480
X default bf800000 bd199998 be0cccce bed11110 bf3dddde bf800000 bf7f7777 c3f00000 c2840001 -2 -481
X default 3e800000 3d888888 3e488888 3eb19999 3eb19999 3e7ddddf 3e800001 42f00000 42bc0000 0 120
X default be800000 3cccccce 3d7ffffe 3d377778 bdb55555 be800000 be7dddde c2f00000 41effffe -1 -121
X default 3f000000 3db33333 3e866666 3efeeeee 3f108888 3efeeef0 3f000001 43700000 42fbffff 0 240
X default 3f7fffef 3e04443e 3ecaaaa2 3f4cccc2 3f7ffff1 3f7f7778 3f800000 43effff0 433dfff8 0 479
X default 3fc00000 3e2eeeee 3f077778 3f8d1110 3fb7bbbc 3fbfbbbc 3fc00000 44340000 437e0001 1 720
X default c3f00000 c21fd110 c2ffbdde c390e6ee c3d0ef76 c3f00001 c3efffbd c8610000 c76fc200 -481 -230401
X default 427a0000 40a82221 41865ddf 4217d333 425a399a 4279fddf 427a0001 46ea6000 45fbf002 62 30000
X default 3a83126f 3d3c131e 3e048a2c 3e4926e9 3e05288d 00000000 3b088889 3ef5c290 42788312 0 0
X default 00000001 3d3bbbbc 3e044444 3e488888 3e044444 00000000 3b088889 000001e0 42780000 0 0
X default 80000001 3d3bbbbc 3e044444 3e488888 3e044444 bb088889 00000000 800001e0 42780000 -1 -1
X default 4f32d05e 4d6e6b27 4e3ebc21 4ed8111b 4f1bb796 4a888889 4a888889 53a7a358 52b2d05f 2147483647 2147483647
X default cf32d05e cd6e6b27 ce3ebc21 ced8111b cf1bb796 4a888889 4a888889 d3a7a358 d2b2d05f 2147483647 2147483647
X default 7f7fffff 7daaaaa9 7e888888 7f1aaaa9 7f5eeeed 4a888889 4a888889 7f800000 7f800000 2147483647 2147483647
X default 7f800000 7f800000 7f800000 7f800000 7f800000 4a888889 4a888889 7f800000 7f800000 2147483647 2147483647
X default ff800000 ff800000 ff800000 ff800000 ff800000 4a888889 4a888889 ff800000 ff800000 2147483647 2147483647
X default 7fc00000 7fc00000 7fc00000 7fc00000 7fc00000 bb088889 00000000 7fc00000 7fc00000 -1 -1
SCALE noarg 00000000 3b088889 bb088889 3e044445 3e066667 be088889 3f2aaaab 3f800000 41077778 4a888889 ca888889
X noarg 00000000 3d3bbbbc 3e044444 3e488888 3e044444 00000000 3b088889 00000000 42780000 0 0
X noarg 80000000 3d3bbbbc 3e044444 3e488888 3e044444 00000000 3b088889 80000000 42780000 0 0
X noarg 3f800000 3e044444 3ecaaaab 3f4ccccc 3f800000 3f7f7778 3f800000 43f00000 433e0000 1 480
X noarg bf800000 bd199998 be0cccce bed11110 bf3dddde bf800000 bf7f7777 c3f00000 c2840001 -2 -481
X noarg 3e800000 3d888888 3e488888 3eb19999 3eb19999 3e7ddddf 3e800001 42f00000 42bc0000 0 120
X noarg be800000 3cccccce 3d7ffffe 3d377778 bdb55555 be800000 be7dddde c2f00000 41effffe -1 -121
X noarg 3f000000 3db33333 3e866666 3efeeeee 3f108888 3efeeef0 3f000001 43700000 42fbffff 0 240
X noarg 3f7fffef 3e04443e 3ecaaaa2 3f4cccc2 3f7ffff1 3f7f7778 3f800000 43effff0 433dfff8 0 479
X noarg 3fc00000 3e2eeeee 3f077778 3f8d1110 3fb7bbbc 3fbfbbbc 3fc00000 44340000 437e0001 1 720
X noarg c3f00000 c21fd110 c2ffbdde c390e6ee c3d0ef76 c3f00001 c3efffbd c8610000 c76fc200 -481 -230401
X noarg 427a0000 40a82221 41865ddf 4217d333 425a399a 4279fddf 427a0001 46ea6000 45fbf002 62 30000
X noarg 3a83126f 3d3c131e 3e048a2c 3e4926e9 3e05288d 00000000 3b088889 3ef5c290 42788312 0 0
X noarg 00000001 3d3bbbbc 3e044444 3e488888 3e044444 00000000 3b088889 000001e0 42780000 0 0
X noarg 80000001 3d3bbbbc 3e044444 3e488888 3e044444 bb088889 00000000 800001e0 42780000 -1 -1
X noarg 4f32d05e 4d6e6b27 4e3ebc21 4ed8111b 4f1bb796 4a888889 4a888889 53a7a358 52b2d05f 2147483647 2147483647
X noarg cf32d05e cd6e6b27 ce3ebc21 ced8111b cf1bb796 4a888889 4a888889 d3a7a358 d2b2d05f 2147483647 2147483647
X noarg 7f7fffff 7daaaaa9 7e888888 7f1aaaa9 7f5eeeed 4a888889 4a888889 7f800000 7f800000 2147483647 2147483647
X noarg 7f800000 7f800000 7f800000 7f800000 7f800000 4a888889 4a888889 7f800000 7f800000 2147483647 2147483647
X noarg ff800000 ff800000 ff800000 ff800000 ff800000 4a888889 4a888889 ff800000 ff800000 2147483647 2147483647
X noarg 7fc00000 7fc00000 7fc00000 7fc00000 7fc00000 bb088889 00000000 7fc00000 7fc00000 -1 -1
SCALE odd 00000000 3b5a740e bb5a740e 3e53a06e 3e570a3e be5a740e 3f888889 3fcccccd 4158bf26 4ada740e cada740e
X odd 00000000 3d888889 3e888889 3eb33334 3e888889 00000000 3b5a740e 00000000 42a00001 0 0
X odd 80000000 3d888889 3e888889 3eb33334 3e888889 00000000 3b5a740e 80000000 42a00001 0 0
X odd 3f800000 3e888889 3f19999a 3f400000 3f800000 3f800000 3f806d3a 43960000 43340000 1 300
X odd bf800000 be08888a bd888888 bd4cccc0 beeeeeee bf806d3a bf800000 c3960000 c19fffff -2 -301
X odd 3e800000 3deeeef0 3eb33334 3ee66667 3ee66667 3e800000 3e81b4e8 42960000 42d20001 0 75
X odd be800000 3c888888 3e3bbbbc 3e800001 3daaaaac be81b4e8 be800000 c2960000 425c0000 -1 -76
X odd 3f000000 3e2aaaac 3eddddde 3f0ccccd 3f222222 3f000000 3f00da74 43160000 43020000 0 150
X odd 3f7fffef 3e888882 3f199994 3f3ffff9 3f7ffff3 3f7f258c 3f800000 4395fff6 4333fff9 0 299
X odd 3fc00000 3ebbbbbc 3f444444 3f733333 3faeeeee 3fc00000 3fc06d3a 43e10000 43660000 1 450
X odd c3f00000 c2bfdddf c31fbbbc c33fa665 c3afddde c3f0006e c3f00001 c80ca000 c73b3000 -481 -144001
X odd 427a0000 41491112 41a8cccd 41cacccc 42386666 427a0000 427a036a 46927c00 45c5d000 62 18750
X odd 3a83126f 3d88f165 3e88b43a 3eb367a2 3e88e8a8 00000000 3b5a740e 3e99999a 42a03334 0 0
X odd 00000001 3d888889 3e888889 3eb33334 3e888889 00000000 3b5a740e 0000012c 42a00001 0 0
X odd 80000001 3d888889 3e888889 3eb33334 3e888889 bb5a740e 00000000 8000012c 42a00001 -1 -1
X odd 4f32d05e 4e0f0d19 4e6e6b28 4e8f0d17 4f032156 4ada740e 4ada740e 53518c2e 528bb2c9 2147483647 2147483647
X odd cf32d05e ce0f0d19 ce6e6b28 ce8f0d17 cf032156 4ada740e 4ada740e d3518c2e d28bb2c9 2147483647 2147483647
X odd 7f7fffff 7e4ccccd 7eaaaaaa 7ecccccb 7f3bbbba 4ada740e 4ada740e 7f800000 7f800000 2147483647 2147483647
X odd 7f800000 7f800000 7f800000 7f800000 7f800000 4ada740e 4ada740e 7f800000 7f800000 2147483647 2147483647
X odd ff800000 ff800000 ff800000 ff800000 ff800000 4ada740e 4ada740e ff800000 ff800000 2147483647 2147483647
X odd 7fc00000 7fc00000 7fc00000 7fc00000 7fc00000 bb5a740e 00000000 7fc00000 7fc00000 -1 -1
";
        let mut out = String::new();
        out.push_str(&format!("DEFAULT {}\n", fields(TerrainLevels::default())));
        out.push_str(&format!(
            "COPY {}\n",
            fields(TerrainLevels::default().copy())
        ));
        let none = TerrainLevels::unconfigured();
        out.push_str(&format!("NOARG {}\n", fields(none)));
        out.push_str(&format!("NOARGCOPY {}\n", fields(none.copy())));
        let (min, max) = (i32::MIN, i32::MAX);
        let cases: [(bool, f32, i32, i32, i32, i32, i32); 19] = [
            (false, 0.0, -9999, 64, 1, 9999, -9999),
            (true, 2.5, -32, 320, 100, 80, 20),
            (true, 1.0, -64, 384, 128, 63, 23),
            (true, 1.0, -64, 320, 128, 63, 23),
            (false, 0.37, 0, 128, 0, 0, 0),
            (true, -3.0, 5, 5000, 5000, 5000, 5000),
            (true, 1.0, -2032, 4064, 4064, 2032, -2032),
            (true, 1.0, -64, 10, 64, 40, 0),
            (true, 1.0, -64, 10, 64, 0, 0),
            (true, 1.0, -64, -100, 64, 40, 0),
            (true, 1.0, -64, -100, 64, 0, 0),
            (true, 1.0, -64, 100_000, 64, 40_000, 0),
            (true, 1.0, -64, max, 64, max, max),
            (true, 1.0, min, min, min, min, min),
            (true, f32::NAN, -64, 480, 128, 62, 22),
            (true, f32::INFINITY, -64, 480, 128, 62, 22),
            (true, -0.0, -64, 480, 128, 62, 22),
            (false, f32::from_bits(1), -64, 480, 128, 62, 61),
            (true, 1.0, -64, 480, 128, 62, 62),
        ];
        for (i, (auto, scale, min_y, max_y, base, sea, floor)) in cases.into_iter().enumerate() {
            let l = TerrainLevels::new(auto, scale, min_y, max_y, base, sea, floor);
            out.push_str(&format!("NEW {i} {}\n", fields(l)));
            out.push_str(&format!("NEWCOPY {i} {}\n", fields(l.copy())));
        }
        let scales = [
            1.0,
            2.5,
            0.0,
            -0.0,
            -1.0,
            f32::NAN,
            f32::INFINITY,
            f32::from_bits(1),
        ];
        for r in [418, 194, 1, 0, -1, -400, max, min] {
            out.push_str(&format!("FREQ {r}"));
            for s in scales {
                out.push_str(&format!(
                    " {} {}",
                    h(NoiseLevels::calc_frequency(r, true, s)),
                    h(NoiseLevels::calc_frequency(r, false, s))
                ));
            }
            out.push('\n');
        }
        transforms(&mut out, "default", TerrainLevels::default());
        transforms(&mut out, "noarg", none);
        transforms(
            &mut out,
            "odd",
            TerrainLevels::new(true, 2.5, -32, 300, 100, 80, 20),
        );
        for (line, (got, want)) in out.lines().zip(EXPECTED.lines()).enumerate() {
            assert_eq!(got, want, "line {}", line + 1);
        }
        assert_eq!(out.lines().count(), EXPECTED.lines().count());
        assert_eq!(
            NoiseLevels::get_default(),
            TerrainLevels::default().noise_levels
        );
    }
}
