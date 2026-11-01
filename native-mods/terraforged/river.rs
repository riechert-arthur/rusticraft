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
    terraforged::{Domain, SourceKind, round},
    terraforged_continent::noise_warp,
    terraforged_engine::{Lake, Levels, Wetland},
};

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

#[cfg(test)]
mod tests {
    use super::super::terraforged_engine::{River, RiverSettings};
    use super::*;

    // Expected values come from a Java oracle run against Engine 0.3.0
    // (`.local/terraforged/oracles/river/RiverConfigOracle.java`).

    /// Engine's `BaseRiverGenerator` main-river config.
    fn main_of(river: River, levels: Levels) -> RiverConfig {
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
    fn fork_of(river: River, levels: Levels) -> RiverConfig {
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
        assert_eq!(River::default(), River::new(0, 0, 0, 0, 0, 0.0));
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
                let river = River::new(a[0], a[1], a[2], a[3], a[4], fade);
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

    #[test]
    fn generation_warps_match_engine_0_3_0() {
        let warp = GenWarp::new(12_345, 3_000);
        assert_eq!(GenWarp::scale_mega_warp(3_000), 1_500);
        assert_eq!(GenWarp::scale_mega_warp(4_500), 2_250);
        assert_eq!(
            [
                (0.0, 0.0),
                (12.25, -98.75),
                (-4096.5, 8192.25),
                (123456.75, -0.125)
            ]
            .map(|(x, z)| [
                warp.lake.x(x, z).to_bits(),
                warp.lake.y(x, z).to_bits(),
                warp.river.x(x, z).to_bits(),
                warp.river.y(x, z).to_bits(),
                warp.lake.offset_x(x, z).to_bits(),
                warp.river.offset_y(x, z).to_bits(),
            ]),
            [
                [0, 0, 0, 0, 0, 0],
                [
                    0xc1d41efe, 0xc2b23d7a, 0x4137be2b, 0xc2c129bf, 0xc21b0f7f, 0x400ac828
                ],
                [
                    0xc582a736, 0x45ffbe18, 0xc5802959, 0x45ffe9d0, 0xc2a8cd8f, 0xc0417f7c
                ],
                [
                    0x47f13e31, 0x4175d274, 0x47f123e3, 0x40d4f618, 0x426e8747, 0x40d8f618
                ],
            ]
        );

        let direct = GenWarp::direct();
        assert_eq!(direct.lake.x(12.25, -98.75).to_bits(), 0x41440000);
        assert_eq!(direct.river.y(12.25, -98.75).to_bits(), 0xc2c58000);
    }
}
