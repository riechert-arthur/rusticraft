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

// Engine 0.3.0's river configuration (`rivermap.river.RiverConfig`, `rivermap.river.Range`,
// `rivermap.lake.LakeConfig`, `rivermap.wetland.WetlandConfig` and `util.Variance`). Engine's
// `BaseRiverGenerator` builds them from `RiverSettings`; TerraForged 0.3.1 itself carves rivers with its own
// continent river config instead, which this module does not port. `GenWarp` (`rivermap.gen.GenWarp`) holds the
// lake and river domain warps Engine's river generators build per continent.

use super::{
    terraforged::{Domain, Line, SourceKind, cos, map, round, simplex, sin},
    terraforged_continent::noise_warp,
    terraforged_engine::{Lake, Levels, Wetland},
    terraforged_fancy::Vec2f,
    terraforged_util::{pack_floats, unpack_left_float, unpack_right_float},
};
use horizon_core::random::{LegacyRandom, RandomSource};

/// Engine's `rivermap.gen.GenWarp`: the lake and river domain warps a river map carves its networks with.
/// `BaseRiverGenerator` and `FancyRiverGenerator` build one per continent from `(int) id`.
#[derive(Clone)]
pub struct GenWarp {
    pub lake: Domain,
    pub river: Domain,
}

impl GenWarp {
    /// `GenWarp.EMPTY`: both warps are `Domain.DIRECT`.
    pub fn direct() -> Self {
        Self {
            lake: Domain::direct(),
            river: Domain::direct(),
        }
    }

    /// `new GenWarp(seed, continentScale)`. Engine ignores `continentScale`. Each warp is
    /// `Domain.warp(++seed, scale, octaves, strength)`: Perlin, x from the seed and y from seed + 1, so one
    /// warp's y source shares its seed with the next warp's x source.
    pub fn new(seed: i32, _continent_scale: i32) -> Self {
        let warp = |n: i32, scale, octaves, strength| {
            noise_warp(
                SourceKind::Perlin,
                seed.wrapping_add(n),
                scale,
                octaves,
                strength,
            )
        };
        Self {
            lake: warp(1, 200, 1, 300.0).add(warp(2, 50, 2, 50.0)),
            river: warp(3, 95, 1, 25.0).add(warp(4, 16, 1, 5.0)),
        }
    }

    /// Engine's private, unused `scaleMegaWarp`: `NoiseUtil.round((float) scale / 3000 * 1500)`.
    pub fn scale_mega_warp(scale: i32) -> i32 {
        round(scale as f32 / 3000.0 * 1500.0)
    }
}

/// Engine's `Variance`: `min + value * range`. `min`, `range` and `of` narrow their `double` arguments to
/// `float` as Engine does; `next(random)` (`apply(random.nextFloat())`) comes with the generators that draw.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Variance {
    min: f32,
    range: f32,
}

impl Variance {
    /// `Variance.min(min)`: `min .. 1`.
    pub const fn min(min: f64) -> Self {
        Self {
            min: min as f32,
            range: 1.0 - min as f32,
        }
    }

    /// `Variance.range(range)`: `1 - range .. 1`.
    pub const fn range(range: f64) -> Self {
        Self {
            min: 1.0 - range as f32,
            range: range as f32,
        }
    }

    /// `Variance.of(min, range)`: the second argument is the *range*, not a maximum.
    pub const fn of(min: f64, range: f64) -> Self {
        Self {
            min: min as f32,
            range: range as f32,
        }
    }

    pub fn apply(self, value: f32) -> f32 {
        self.min + value * self.range
    }

    /// `apply(value, scaler)`.
    pub fn apply_scaled(self, value: f32, scaler: f32) -> f32 {
        self.apply(value) * scaler
    }
}

/// Engine's `rivermap.river.Range`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Range {
    pub min: f32,
    pub max: f32,
}

impl Range {
    pub const fn new(min: f32, max: f32) -> Self {
        Self { min, max }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec2i {
    pub x: i32,
    pub y: i32,
}

impl Vec2i {
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct River {
    pub x1: f32,
    pub z1: f32,
    pub x2: f32,
    pub z2: f32,
    pub dx: f32,
    pub dz: f32,
    pub ndx: f32,
    pub ndz: f32,
    pub norm_x: f32,
    pub norm_z: f32,
    pub length: f32,
    pub length2: f32,
    pub min_x: f32,
    pub min_z: f32,
    pub max_x: f32,
    pub max_z: f32,
}

impl River {
    pub const VALLEY_WIDTH: i32 = 275;
    pub const MAIN_VALLEY: Variance = Variance::of(0.8, 0.7);
    pub const FORK_VALLEY: Variance = Variance::of(0.4, 0.75);
    pub const FORK_ANGLE: Variance = Variance::of(0.075, 0.115);
    pub const MAIN_SPACING: Variance = Variance::of(0.1, 0.25);
    pub const FORK_SPACING: Variance = Variance::of(0.25, 0.25);

    pub fn new(x1: f32, z1: f32, x2: f32, z2: f32) -> Self {
        Self::with_radius(x1, z1, x2, z2, Self::VALLEY_WIDTH as f32)
    }

    pub fn with_radius(x1: f32, z1: f32, x2: f32, z2: f32, radius: f32) -> Self {
        let radius = radius * 2.0;
        let dx = x2 - x1;
        let dz = z2 - z1;
        let length = f64::from(dx * dx + dz * dz).sqrt() as f32;
        let ndx = dx / length;
        let ndz = dz / length;
        Self {
            x1,
            z1,
            x2,
            z2,
            dx,
            dz,
            ndx,
            ndz,
            norm_x: ndz,
            norm_z: -ndx,
            length,
            length2: length * length,
            min_x: x1.min(x2) - radius,
            min_z: z1.min(z2) - radius,
            max_x: x1.max(x2) + radius,
            max_z: z1.max(z2) + radius,
        }
    }

    pub const fn length(self) -> f32 {
        self.length
    }

    pub fn angle(self) -> f32 {
        f64::from(self.dx).atan2(f64::from(self.dz)) as f32
    }

    pub fn pos(self, distance: f32) -> i64 {
        pack_floats(self.x1 + self.dx * distance, self.z1 + self.dz * distance)
    }

    pub fn warped_pos(self, distance: f32, warp: RiverWarp) -> i64 {
        let mut x = self.x1 + self.dx * distance;
        let mut z = self.z1 + self.dz * distance;
        if warp.test(distance) {
            let offset = warp.offset(x, z, distance, self);
            x -= unpack_left_float(offset);
            z -= unpack_right_float(offset);
        }
        pack_floats(x, z)
    }

    pub fn intersects(self, other: Self) -> bool {
        Line::intersect(
            other.x1, other.z1, other.x2, other.z2, self.x1, self.z1, self.x2, self.z2,
        )
    }

    pub fn intersects_extended(self, other: Self, extend: f32) -> bool {
        let extend_a = clamp(extend / self.length, 0.0, 1.0);
        let extend_b = clamp(extend / other.length, 0.0, 1.0);
        Line::intersect(
            self.x1 - self.dx * extend_a,
            self.z1 - self.dz * extend_a,
            self.x2 + self.dx * extend_a,
            self.z2 + self.dz * extend_a,
            other.x1 - other.dx * extend_b,
            other.z1 - other.dz * extend_b,
            other.x2 + other.dx * extend_b,
            other.z2 + other.dz * extend_b,
        )
    }

    pub const fn contains(self, x: f32, z: f32) -> bool {
        x >= self.min_x && x <= self.max_x && z >= self.min_z && z <= self.max_z
    }

    pub fn overlaps(self, other: Self) -> bool {
        self.overlaps_bounds(other.min_x, other.min_z, other.max_x, other.max_z)
    }

    pub const fn overlaps_bounds(self, min_x: f32, min_y: f32, max_x: f32, max_y: f32) -> bool {
        self.min_x < max_x && self.max_x > min_x && self.min_z < max_y && self.max_z > min_y
    }

    pub const fn overlaps_circle(self, center: Vec2f, radius: f32) -> bool {
        self.overlaps_bounds(
            center.x - radius,
            center.y - radius,
            center.x + radius,
            center.y + radius,
        )
    }

    pub fn shorten(self, distance: i32) -> Self {
        let factor = distance as f32 / self.length;
        Self::new(
            round(self.x1 + self.dx * factor) as f32,
            round(self.z1 + self.dz * factor) as f32,
            self.x2,
            self.z2,
        )
    }

    pub fn from_nodes(first: Vec2i, second: Vec2i) -> Self {
        Self::with_radius(
            first.x as f32,
            first.y as f32,
            second.x as f32,
            second.y as f32,
            300.0,
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RiverWarp {
    seed: i32,
    lower: f32,
    upper: f32,
    lower_range: f32,
    upper_range: f32,
    frequency: f32,
    scale: f32,
    enabled: bool,
}

impl RiverWarp {
    pub const fn none() -> Self {
        Self {
            seed: 0,
            lower: 0.0,
            upper: 0.0,
            lower_range: f32::INFINITY,
            upper_range: 1.0,
            frequency: 0.0,
            scale: 0.0,
            enabled: false,
        }
    }

    pub fn new(seed: i32, lower: f32, upper: f32, frequency: f32, scale: f32) -> Self {
        Self {
            seed,
            lower,
            upper,
            lower_range: 1.0 / lower,
            upper_range: 1.0 / (1.0 - upper),
            frequency,
            scale,
            enabled: true,
        }
    }

    pub fn child(self, lower: f32, upper: f32, factor: f32, random: &mut LegacyRandom) -> Self {
        Self::new(
            random.next_int(),
            lower,
            upper,
            self.frequency * factor,
            self.scale * factor,
        )
    }

    pub const fn test(self, distance: f32) -> bool {
        self.enabled && distance >= 0.0 && distance <= 1.0
    }

    pub fn offset(self, x: f32, z: f32, distance: f32, river: River) -> i64 {
        let noise = simplex(x * self.frequency, z * self.frequency, self.seed);
        let warp_distance = self.warp_alpha(distance) * self.scale;
        let mut dx = river.norm_x * noise * warp_distance;
        let mut dz = river.norm_z * noise * warp_distance;
        let factor = river.length * 0.0004;
        let wiggle_frequency = 8.0 * factor;
        let wiggle_distance = clamp(self.wiggle_alpha(distance) * 25.0 * factor, 2.0, 45.0);
        let radians = noise + distance * java_pi2() * wiggle_frequency;
        dx += cos(radians) * river.norm_x * wiggle_distance;
        dz += sin(radians) * river.norm_z * wiggle_distance;
        pack_floats(dx, dz)
    }

    pub fn create(fade: f32, random: &mut LegacyRandom) -> Self {
        Self::create_range(fade, 1.0 - fade, random)
    }

    pub fn create_range(lower: f32, upper: f32, random: &mut LegacyRandom) -> Self {
        let scale = 125.0 + random.next_int_bound(50).expect("positive bound") as f32;
        let frequency = 0.0005 + random.next_float() * 0.0005;
        Self::new(random.next_int(), lower, upper, frequency, scale)
    }

    fn warp_alpha(self, distance: f32) -> f32 {
        if !(0.0..=1.0).contains(&distance) {
            0.0
        } else if distance < self.lower {
            distance * self.lower_range
        } else if distance > self.upper {
            (1.0 - distance) * self.upper_range
        } else {
            1.0
        }
    }

    fn wiggle_alpha(self, distance: f32) -> f32 {
        map(distance, 0.0, 0.075, 0.075)
    }
}

/// Engine's `RiverConfig`: widths in blocks, heights normalized by `Levels`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RiverConfig {
    pub order: i32,
    pub main: bool,
    pub bed_width: i32,
    pub bank_width: i32,
    pub bed_height: f32,
    pub min_bank_height: f32,
    pub max_bank_height: f32,
    pub length: i32,
    pub length2: i32,
    pub fade: f32,
}

impl RiverConfig {
    pub const fn builder(levels: Levels) -> RiverConfigBuilder {
        RiverConfigBuilder::new(levels)
    }

    /// `createFork(connectWidth, levels)`: itself when its bank is narrower than `connect_width`, else a fork
    /// scaled down to that width.
    pub fn create_fork(self, connect_width: f32, levels: Levels) -> Self {
        if (self.bank_width as f32) < connect_width {
            return self;
        }
        let scale = self.bank_width as f32 / connect_width;
        self.create_fork_sized(
            levels.scale_value(self.bed_height),
            round(self.bed_width as f32 / scale),
            round(self.bank_width as f32 / scale),
            levels,
        )
    }

    /// `createFork(bedHeight, bedWidth, bankWidth, levels)`: banks one block lower (at least ground level),
    /// `order + 1`, never main; `length`, `length2` and `fade` are kept.
    pub fn create_fork_sized(
        self,
        bed_height: i32,
        bed_width: i32,
        bank_width: i32,
        levels: Levels,
    ) -> Self {
        let min_bank_height = levels
            .ground_level
            .max(levels.scale_value(self.min_bank_height).wrapping_sub(1));
        let max_bank_height =
            min_bank_height.max(levels.scale_value(self.max_bank_height).wrapping_sub(1));
        Self {
            main: false,
            order: self.order.wrapping_add(1),
            bed_width,
            bank_width,
            bed_height: levels.scale_level(bed_height),
            min_bank_height: levels.scale_level(min_bank_height),
            max_bank_height: levels.scale_level(max_bank_height),
            length: self.length,
            length2: self.length2,
            fade: self.fade,
        }
    }
}

/// Engine's `RiverConfig.Builder`, with its defaults.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RiverConfigBuilder {
    main: bool,
    order: i32,
    bed_width: i32,
    bank_width: i32,
    bed_depth: i32,
    max_bank_height: i32,
    min_bank_height: i32,
    length: i32,
    fade: f64,
    levels: Levels,
}

impl RiverConfigBuilder {
    pub const fn new(levels: Levels) -> Self {
        Self {
            main: false,
            order: 0,
            bed_width: 4,
            bank_width: 15,
            bed_depth: 5,
            max_bank_height: 1,
            min_bank_height: 1,
            length: 1000,
            fade: 0.2,
            levels,
        }
    }

    pub const fn order(mut self, value: i32) -> Self {
        self.order = value;
        self
    }
    pub const fn main(mut self, value: bool) -> Self {
        self.main = value;
        self
    }
    pub const fn bed_width(mut self, value: i32) -> Self {
        self.bed_width = value;
        self
    }
    pub const fn bank_width(mut self, value: i32) -> Self {
        self.bank_width = value;
        self
    }
    pub const fn bed_depth(mut self, value: i32) -> Self {
        self.bed_depth = value;
        self
    }
    pub const fn length(mut self, value: i32) -> Self {
        self.length = value;
        self
    }
    /// Engine's builder keeps `fade` as a `double`; callers passing a `float` setting widen it.
    pub const fn fade(mut self, value: f64) -> Self {
        self.fade = value;
        self
    }

    /// `bankHeight(min, max)`: sorts its arguments.
    pub const fn bank_height(mut self, min: i32, max: i32) -> Self {
        self.min_bank_height = if min < max { min } else { max };
        self.max_bank_height = if min > max { min } else { max };
        self
    }

    pub fn build(self) -> RiverConfig {
        RiverConfig {
            main: self.main,
            order: self.order,
            bed_width: self.bed_width,
            bank_width: self.bank_width,
            bed_height: self.levels.water(self.bed_depth.wrapping_neg()),
            min_bank_height: self.levels.water(self.min_bank_height),
            max_bank_height: self.levels.water(self.max_bank_height),
            length: self.length,
            length2: self.length.wrapping_mul(self.length),
            fade: self.fade as f32,
        }
    }
}

/// Engine's `LakeConfig` (`LakeConfig.of(settings, levels)`; the builder's own defaults are always
/// overwritten by `of`, so they are not ported).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LakeConfig {
    pub depth: f32,
    pub chance: f32,
    pub size_min: f32,
    pub size_max: f32,
    pub size_range: f32,
    pub bank_min: f32,
    pub bank_max: f32,
    pub distance_min: f32,
    pub distance_max: f32,
}

impl LakeConfig {
    pub fn new(settings: Lake, levels: Levels) -> Self {
        let size_min = settings.size_min as f32;
        let size_max = settings.size_max as f32;
        Self {
            depth: levels.water(settings.depth.wrapping_neg()),
            chance: settings.chance,
            size_min,
            size_max,
            // Engine subtracts the `float` fields, not the `int` settings.
            size_range: size_max - size_min,
            bank_min: levels.water(settings.min_bank_height),
            bank_max: levels.water(settings.max_bank_height),
            distance_min: settings.min_start_distance,
            distance_max: settings.max_start_distance,
        }
    }
}

/// Engine's `WetlandConfig`. As in Engine, `length` is `Variance.of(sizeMin, sizeMax)`: it spans
/// `sizeMin .. sizeMin + sizeMax`, not `sizeMin .. sizeMax`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WetlandConfig {
    pub skip_size: i32,
    pub length: Variance,
    pub width: Variance,
}

impl WetlandConfig {
    pub fn new(settings: Wetland) -> Self {
        Self {
            skip_size: 1.max(round((1.0 - settings.chance) * 10.0)),
            length: Variance::of(f64::from(settings.size_min), f64::from(settings.size_max)),
            width: Variance::of(50.0, 150.0),
        }
    }
}

fn clamp(value: f32, min: f32, max: f32) -> f32 {
    value.max(min).min(max)
}

fn java_pi2() -> f32 {
    f32::from_bits(0x40c9_0fdb)
}

#[cfg(test)]
mod tests {
    use super::super::terraforged_engine::{River as EngineRiver, RiverSettings};
    use super::*;

    // Expected values come from a Java oracle run against Engine 0.3.0
    // (`.local/terraforged/oracles/river/RiverConfigOracle.java`).

    /// Engine's `BaseRiverGenerator` main-river config.
    fn main_of(river: EngineRiver, levels: Levels) -> RiverConfig {
        RiverConfig::builder(levels)
            .bank_height(river.min_bank_height, river.max_bank_height)
            .bank_width(river.bank_width)
            .bed_width(river.bed_width)
            .bed_depth(river.bed_depth)
            .fade(f64::from(river.fade))
            .length(5000)
            .main(true)
            .order(0)
            .build()
    }

    /// Engine's `BaseRiverGenerator` fork config.
    fn fork_of(river: EngineRiver, levels: Levels) -> RiverConfig {
        RiverConfig::builder(levels)
            .bank_height(river.min_bank_height, river.max_bank_height)
            .bank_width(river.bank_width)
            .bed_width(river.bed_width)
            .bed_depth(river.bed_depth)
            .fade(f64::from(river.fade))
            .length(4500)
            .order(1)
            .build()
    }

    /// Java's `Float.floatToIntBits`: one canonical NaN (NaN signs from `inf - inf` and `0 * inf` differ
    /// between HotSpot and Rust).
    fn bits(value: f32) -> u32 {
        if value.is_nan() {
            0x7fc0_0000
        } else {
            value.to_bits()
        }
    }

    fn river_fields(c: RiverConfig) -> [u32; 10] {
        [
            u32::from(c.main),
            c.order as u32,
            c.bed_width as u32,
            c.bank_width as u32,
            bits(c.bed_height),
            bits(c.min_bank_height),
            bits(c.max_bank_height),
            c.length as u32,
            c.length2 as u32,
            bits(c.fade),
        ]
    }

    fn lake_fields(l: LakeConfig) -> [u32; 9] {
        [
            l.depth,
            l.chance,
            l.size_min,
            l.size_max,
            l.size_range,
            l.bank_min,
            l.bank_max,
            l.distance_min,
            l.distance_max,
        ]
        .map(bits)
    }

    fn variance_fields(v: Variance) -> [u32; 2] {
        [bits(v.min), bits(v.range)]
    }

    /// Java's `fold = fold * 31 + v` over `int`s.
    struct Fold(i32);

    impl Fold {
        fn new() -> Self {
            Self(17)
        }
        fn mix(&mut self, values: impl IntoIterator<Item = u32>) {
            for v in values {
                self.0 = self.0.wrapping_mul(31).wrapping_add(v as i32);
            }
        }
    }

    #[test]
    fn engine_default_settings() {
        let settings = RiverSettings::default();
        assert_eq!((settings.seed_offset, settings.river_count), (0, 8));
        let main = settings.main_rivers;
        assert_eq!(
            (
                main.bed_depth,
                main.min_bank_height,
                main.max_bank_height,
                main.bed_width,
                main.bank_width
            ),
            (5, 2, 6, 8, 20)
        );
        assert_eq!(main.fade.to_bits(), 0x3f400000);
        let branch = settings.branch_rivers;
        assert_eq!(
            (
                branch.bed_depth,
                branch.min_bank_height,
                branch.max_bank_height,
                branch.bed_width,
                branch.bank_width
            ),
            (4, 1, 4, 5, 14)
        );
        assert_eq!(branch.fade.to_bits(), 0x3f79999a);
        assert_eq!(EngineRiver::default(), EngineRiver::new(0, 0, 0, 0, 0, 0.0));
        let lake = settings.lakes;
        assert_eq!(
            [
                lake.chance,
                lake.min_start_distance,
                lake.max_start_distance
            ]
            .map(f32::to_bits),
            [0x3e99999a, 0, 0x3cf5c28f]
        );
        assert_eq!(
            (
                lake.depth,
                lake.size_min,
                lake.size_max,
                lake.min_bank_height,
                lake.max_bank_height
            ),
            (10, 75, 150, 2, 10)
        );
        let wetland = settings.wetlands;
        assert_eq!(wetland.chance.to_bits(), 0x3f19999a);
        assert_eq!((wetland.size_min, wetland.size_max), (175, 225));
    }

    #[test]
    fn engine_default_configs() {
        let levels = Levels::new(256, 63);
        let settings = RiverSettings::default();
        assert_eq!(
            river_fields(RiverConfig::builder(levels).build()),
            [
                0, 0, 4, 15, 0x3e640000, 0x3e7c0000, 0x3e7c0000, 1000, 1_000_000, 0x3e4ccccd
            ]
        );
        let main = main_of(settings.main_rivers, levels);
        assert_eq!(
            river_fields(main),
            [
                1, 0, 8, 20, 0x3e640000, 0x3e800000, 0x3e880000, 5000, 25_000_000, 0x3f400000
            ]
        );
        assert_eq!(
            river_fields(fork_of(settings.branch_rivers, levels)),
            [
                0, 1, 5, 14, 0x3e680000, 0x3e7c0000, 0x3e840000, 4500, 20_250_000, 0x3f79999a
            ]
        );
        assert_eq!(
            river_fields(main.create_fork(10.0, levels)),
            [
                0, 1, 4, 10, 0x3e640000, 0x3e800000, 0x3e860000, 5000, 25_000_000, 0x3f400000
            ]
        );
        assert_eq!(
            lake_fields(LakeConfig::new(settings.lakes, levels)),
            [
                0x3e500000, 0x3e99999a, 0x42960000, 0x43160000, 0x42960000, 0x3e800000, 0x3e900000,
                0, 0x3cf5c28f
            ]
        );
        let wetland = WetlandConfig::new(settings.wetlands);
        assert_eq!(wetland.skip_size, 4);
        assert_eq!(variance_fields(wetland.length), [0x432f0000, 0x43610000]);
        assert_eq!(variance_fields(wetland.width), [0x42480000, 0x43160000]);
        assert_eq!(Variance::min(0.25).apply(0.5).to_bits(), 0x3f200000);
        assert_eq!(
            Variance::range(0.25).apply_scaled(0.5, 2.0).to_bits(),
            0x3fe00000
        );
    }

    fn edge_levels() -> [Levels; 6] {
        [
            Levels::new(256, 63),
            Levels::new(63, 255),
            Levels::new(1, 0),
            Levels::new(-10, -5),
            Levels::new(384, i32::MIN),
            Levels::new(i32::MAX, i32::MAX),
        ]
    }

    #[test]
    fn river_configs_fold_like_engine() {
        let rivers = [
            [5, 2, 6, 20, 8],
            [4, 1, 4, 14, 5],
            [0, 0, 0, 0, 0],
            [i32::MIN, i32::MAX, i32::MIN, 7, 3],
            [10, 10, 1, 50, 20],
            [-3, 5, 2, 1, -1],
            [1, 0, 0, i32::MAX, i32::MIN],
        ];
        let fades = [0.75, 0.975, 0.0, f32::NAN, 1.0, -0.5, f32::from_bits(1)];
        let connect = [
            10.0,
            0.5,
            0.0,
            -0.0,
            -4.0,
            f32::NAN,
            1e30,
            f32::INFINITY,
            20.0,
            3.0,
        ];
        let sized = [[0, 0, 0], [i32::MIN, i32::MAX, -1], [64, 3, 7]];
        let mut fold = Fold::new();
        for levels in edge_levels() {
            for (a, fade) in rivers.iter().zip(fades) {
                let river = EngineRiver::new(a[0], a[1], a[2], a[3], a[4], fade);
                let configs = [
                    main_of(river, levels),
                    fork_of(river, levels),
                    RiverConfig::builder(levels).build(),
                    RiverConfig::builder(levels)
                        .length(i32::MAX)
                        .fade(1e300)
                        .order(i32::MAX)
                        .build(),
                ];
                for config in configs {
                    fold.mix(river_fields(config));
                    for width in connect {
                        let fork = config.create_fork(width, levels);
                        fold.mix(river_fields(fork));
                        fold.mix(river_fields(fork.create_fork(width, levels)));
                    }
                    for [bed_height, bed_width, bank_width] in sized {
                        fold.mix(river_fields(
                            config.create_fork_sized(bed_height, bed_width, bank_width, levels),
                        ));
                    }
                }
            }
        }
        assert_eq!(fold.0, 1410638671);
    }

    const CHANCES: [f32; 10] = [
        0.3,
        0.0,
        1.0,
        f32::NAN,
        -2.0,
        0.35,
        0.65,
        1e10,
        0.95,
        0.949999,
    ];
    const SIZES: [[i32; 5]; 5] = [
        [10, 75, 150, 2, 10],
        [i32::MIN, 0, i32::MAX, -1, i32::MAX],
        [0, 16_777_217, 1, 0, 0],
        [-5, i32::MAX, -1, i32::MIN, 3],
        [1, 33_554_431, 33_554_433, 1, 1],
    ];

    #[test]
    fn lake_configs_fold_like_engine() {
        let distances = [0.0, 0.03, -0.0, f32::NAN, 1e30];
        let mut fold = Fold::new();
        for levels in edge_levels() {
            for a in SIZES {
                for (c, chance) in CHANCES.into_iter().enumerate() {
                    let lake = Lake {
                        chance,
                        min_start_distance: distances[c % distances.len()],
                        max_start_distance: distances[(c + 1) % distances.len()],
                        depth: a[0],
                        size_min: a[1],
                        size_max: a[2],
                        min_bank_height: a[3],
                        max_bank_height: a[4],
                    };
                    fold.mix(lake_fields(LakeConfig::new(lake, levels)));
                }
            }
        }
        assert_eq!(fold.0, 2121404999);
    }

    #[test]
    fn wetland_configs_fold_like_engine() {
        let mut fold = Fold::new();
        for chance in CHANCES {
            for a in SIZES {
                let config = WetlandConfig::new(Wetland {
                    chance,
                    size_min: a[1],
                    size_max: a[2],
                });
                fold.mix([config.skip_size as u32]);
                fold.mix(variance_fields(config.length));
                fold.mix(variance_fields(config.width));
            }
        }
        assert_eq!(fold.0, 2121468570);
    }

    #[test]
    fn variances_fold_like_engine() {
        let ds = [
            0.25,
            0.0,
            -0.0,
            1.0,
            f64::from(1.99_f32),
            0.1,
            f64::NAN,
            1e300,
            -1e-300,
            3.4028235677973366e38,
            f64::INFINITY,
        ];
        let values = [0.5, 0.0, 1.0, -1.0, f32::NAN, 0.1];
        let mut fold = Fold::new();
        for a in ds {
            for b in ds {
                for v in [Variance::min(a), Variance::range(a), Variance::of(a, b)] {
                    fold.mix(variance_fields(v));
                    for x in values {
                        fold.mix([bits(v.apply(x)), bits(v.apply_scaled(x, b as f32))]);
                    }
                }
            }
        }
        assert_eq!(fold.0, 265982105);
    }

    // Expected values from `.local/terraforged/oracles/river/GenWarpOracle.java`.
    const WARP_COORDS: [f32; 22] = [
        0.0,
        -0.0,
        12.25,
        -98.75,
        -4096.5,
        8192.25,
        123456.75,
        -0.125,
        1e-3,
        3.0e7,
        -3.0e7,
        16_777_217.0,
        1e20,
        -1e30,
        f32::MAX,
        -f32::MAX,
        f32::from_bits(1),
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NAN,
        2_147_483_647.0,
        -2_147_483_648.0,
    ];

    fn mix_domain(fold: &mut Fold, domain: &Domain, x: f32, z: f32) {
        fold.mix([
            bits(domain.offset_x(x, z)),
            bits(domain.offset_y(x, z)),
            bits(domain.x(x, z)),
            bits(domain.y(x, z)),
        ]);
    }

    #[test]
    fn gen_warp_is_shareable_across_workers() {
        fn shareable<T: Send + Sync>(_: &T) {}
        shareable(&GenWarp::new(12_345, 3000));
    }

    #[test]
    fn scale_mega_warp_rounds_like_engine() {
        let scales = [
            0,
            1,
            2,
            3,
            -1,
            -2,
            -3,
            3000,
            4500,
            -4500,
            1001,
            -1001,
            7,
            -7,
            16_777_217,
            i32::MAX,
            i32::MIN,
            123_456_789,
        ];
        let rounded = scales.map(GenWarp::scale_mega_warp);
        assert_eq!(
            rounded,
            [
                0,
                1,
                1,
                2,
                -1,
                -1,
                -2,
                1500,
                2250,
                -2250,
                500,
                -500,
                3,
                -3,
                8_388_608,
                1_073_741_824,
                -1_073_741_824,
                61_728_400
            ]
        );
    }

    #[test]
    fn empty_warp_is_direct() {
        let warp = GenWarp::direct();
        let mut fold = Fold::new();
        for domain in [&warp.lake, &warp.river] {
            for x in WARP_COORDS {
                for z in WARP_COORDS {
                    mix_domain(&mut fold, domain, x, z);
                }
            }
        }
        assert_eq!(fold.0, 215664657);
    }

    #[test]
    fn generation_warps_fold_like_engine() {
        let cases = [
            (0, -515557086),
            (12_345, -949602364),
            (-1, -1524105944),
            (1, -359467591),
            (i32::MAX, -1524105944),
            (i32::MAX - 2, 887431308),
            (i32::MIN, -515557086),
            (-987_654_321, -1312107752),
        ];
        for (seed, expected) in cases {
            let warp = GenWarp::new(seed, 3000);
            let mut fold = Fold::new();
            for x in WARP_COORDS {
                for z in WARP_COORDS {
                    mix_domain(&mut fold, &warp.lake, x, z);
                    mix_domain(&mut fold, &warp.river, x, z);
                }
            }
            assert_eq!(fold.0, expected, "seed {seed}");
        }
        let warp = GenWarp::new(12_345, 3000);
        assert_eq!(
            [
                warp.lake.x(12.25, -98.75),
                warp.lake.y(12.25, -98.75),
                warp.river.x(12.25, -98.75),
                warp.river.y(12.25, -98.75)
            ]
            .map(f32::to_bits),
            [0xc1d41efe, 0xc2b23d7a, 0x4137be2b, 0xc2c129bf]
        );
        // Engine ignores `continentScale`.
        let other = GenWarp::new(12_345, i32::MIN);
        assert_eq!(
            other.lake.x(5.0, 7.0).to_bits(),
            GenWarp::new(12_345, 0).lake.x(5.0, 7.0).to_bits()
        );
    }

    #[test]
    fn river_geometry_matches_engine_0_3_0() {
        let river = River::with_radius(10.25, -20.5, 310.75, 179.125, 275.0);
        assert_eq!(
            [
                river.dx,
                river.dz,
                river.ndx,
                river.ndz,
                river.norm_x,
                river.norm_z,
                river.length,
                river.length2,
                river.min_x,
                river.min_z,
                river.max_x,
                river.max_z,
                river.angle(),
                river.length()
            ]
            .map(f32::to_bits),
            [
                0x43964000, 0x4347a000, 0x3f553c91, 0x3f0da7b4, 0x3f0da7b4, 0xbf553c91, 0x43b461be,
                0x47fe3331, 0xc406f000, 0xc40ea000, 0x44573000, 0x44364800, 0x3f7c036f, 0x43b461be
            ]
        );
        assert_eq!(
            [-0.25, 0.0, 0.25, 1.0, 1.25].map(|distance| river.pos(distance) as u64),
            [
                0xc281c000c28cd000,
                0x41240000c1a40000,
                0x42aac00041eb4000,
                0x439b600043332000,
                0x43c0f00043650800
            ]
        );

        let crossing = River::new(100.0, 250.0, 200.0, -100.0);
        let far = River::new(1000.0, 1000.0, 1100.0, 1200.0);
        assert_eq!(
            [
                river.contains(0.0, 0.0),
                river.contains(900.0, 900.0),
                river.intersects(crossing),
                river.intersects(far),
                river.intersects_extended(far, 1000.0),
                river.overlaps(far)
            ],
            [true, false, true, false, false, true]
        );
        let shortened = river.shorten(100);
        assert_eq!(
            [
                shortened.x1,
                shortened.z1,
                shortened.x2,
                shortened.z2,
                shortened.length
            ]
            .map(f32::to_bits),
            [0x42bc0000, 0x420c0000, 0x439b6000, 0x43332000, 0x43822589]
        );

        let warp = RiverWarp::new(918_273, 0.2, 0.8, 0.00075, 140.0);
        assert_eq!(
            [-0.1, 0.0, 0.1, 0.2, 0.5, 0.8, 0.9, 1.0, 1.1].map(|distance| {
                let x = river.x1 + river.dx * distance;
                let z = river.z1 + river.dz * distance;
                (
                    warp.test(distance),
                    warp.offset(x, z, distance, river) as u64,
                    river.warped_pos(distance, warp) as u64,
                )
            }),
            [
                (false, 0x3f5bb07b3f86638d, 0xc19e6667c221d99a),
                (true, 0x3f8d012bbe234ccc, 0x41125fdbc1a2b966),
                (true, 0x40e2ee0cc130e517, 0x4204d57241284b7d),
                (true, 0x4176857dc1d37173, 0x425bc50942376bec),
                (true, 0x41bae21ec20c5577, 0x430923bc42e4cabc),
                (true, 0x41f6b119c22c6d9d, 0x435bd04443364e9a),
                (true, 0x4183473ec1c03db8, 0x4384252543373150),
                (true, 0x3ed47388c03c1e90, 0x439b2ae34336107a),
                (false, 0xbf812bfbc025ef9c, 0x43aa666743471667),
            ]
        );
        assert!(!RiverWarp::none().test(0.5));
        assert!(RiverWarp::new(0, 0.0, 0.0, 0.0, 0.0).test(0.5));
    }
}
