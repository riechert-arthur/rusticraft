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
    terraforged::{Domain, Interpolation, Module, Source, SourceConfig, SourceKind},
    terraforged_engine::{Lake, Levels, River, Wetland},
};

#[derive(Clone)]
pub struct GenWarp {
    pub lake: Domain,
    pub river: Domain,
}

impl GenWarp {
    pub fn direct() -> Self {
        Self {
            lake: Domain::direct(),
            river: Domain::direct(),
        }
    }

    pub fn new(mut seed: i32, _continent_scale: i32) -> Self {
        seed = seed.wrapping_add(1);
        let lake = noise_warp(seed, 200, 1, 300.0);
        seed = seed.wrapping_add(1);
        let lake = lake.add(noise_warp(seed, 50, 2, 50.0));
        seed = seed.wrapping_add(1);
        let river = noise_warp(seed, 95, 1, 25.0);
        seed = seed.wrapping_add(1);
        let river = river.add(noise_warp(seed, 16, 1, 5.0));
        Self { lake, river }
    }

    pub fn scale_mega_warp(scale: i32) -> i32 {
        noise_round(scale as f32 / 3000.0 * 1500.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Variance {
    min: f32,
    range: f32,
}

impl Variance {
    pub const fn min(min: f64) -> Self {
        Self {
            min: min as f32,
            range: 1.0 - min as f32,
        }
    }

    pub const fn range(range: f64) -> Self {
        Self {
            min: 1.0 - range as f32,
            range: range as f32,
        }
    }

    pub const fn of(min: f64, range: f64) -> Self {
        Self {
            min: min as f32,
            range: range as f32,
        }
    }

    pub fn apply(self, value: f32) -> f32 {
        self.min + value * self.range
    }

    pub fn apply_scaled(self, value: f32, scaler: f32) -> f32 {
        self.apply(value) * scaler
    }
}

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

    pub fn from_settings(settings: River, levels: Levels, main: bool, order: i32) -> Self {
        Self::builder(levels)
            .main(main)
            .order(order)
            .bed_width(settings.bed_width)
            .bank_width(settings.bank_width)
            .bed_depth(settings.bed_depth)
            .bank_height(settings.min_bank_height, settings.max_bank_height)
            .fade(f64::from(settings.fade))
            .build()
    }

    pub fn create_fork(self, connect_width: f32, levels: Levels) -> Self {
        if (self.bank_width as f32) < connect_width {
            return self;
        }
        let scale = self.bank_width as f32 / connect_width;
        self.create_fork_sized(
            levels.scale_value(self.bed_height),
            noise_round(self.bed_width as f32 / scale),
            noise_round(self.bank_width as f32 / scale),
            levels,
        )
    }

    pub fn create_fork_sized(
        self,
        bed_height: i32,
        bed_width: i32,
        bank_width: i32,
        levels: Levels,
    ) -> Self {
        let min_bank_height = levels
            .ground_level
            .max(levels.scale_value(self.min_bank_height) - 1);
        let max_bank_height = min_bank_height.max(levels.scale_value(self.max_bank_height) - 1);
        Self {
            main: false,
            order: self.order + 1,
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
    pub const fn fade(mut self, value: f64) -> Self {
        self.fade = value;
        self
    }

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
            bed_height: self.levels.water(-self.bed_depth),
            min_bank_height: self.levels.water(self.min_bank_height),
            max_bank_height: self.levels.water(self.max_bank_height),
            length: self.length,
            length2: self.length.wrapping_mul(self.length),
            fade: self.fade as f32,
        }
    }
}

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
        Self {
            depth: levels.water(-settings.depth),
            chance: settings.chance,
            size_min: settings.size_min as f32,
            size_max: settings.size_max as f32,
            size_range: settings.size_max.wrapping_sub(settings.size_min) as f32,
            bank_min: levels.water(settings.min_bank_height),
            bank_max: levels.water(settings.max_bank_height),
            distance_min: settings.min_start_distance,
            distance_max: settings.max_start_distance,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WetlandConfig {
    pub skip_size: i32,
    pub length: Variance,
    pub width: Variance,
}

impl WetlandConfig {
    pub fn new(settings: Wetland) -> Self {
        Self {
            skip_size: 1.max(noise_round((1.0 - settings.chance) * 10.0)),
            length: Variance::of(f64::from(settings.size_min), f64::from(settings.size_max)),
            width: Variance::of(50.0, 150.0),
        }
    }
}

fn noise_round(value: f32) -> i32 {
    (value + 0.5) as i32
}

fn noise_warp(seed: i32, scale: i32, octaves: usize, strength: f32) -> Domain {
    let source = |seed| {
        Module::source(Source::new(
            SourceKind::Perlin,
            SourceConfig {
                seed,
                octaves,
                gain: 0.5,
                frequency: 1.0 / scale as f32,
                lacunarity: 2.0,
                interpolation: Interpolation::Curve3,
            },
        ))
    };
    Domain::domain_warp(
        source(seed),
        source(seed.wrapping_add(1)),
        Module::constant(strength),
    )
}

#[cfg(test)]
mod tests {
    use super::super::terraforged_engine::RiverSettings;
    use super::*;

    fn river_bits(config: RiverConfig) -> [u32; 4] {
        [
            config.bed_height.to_bits(),
            config.min_bank_height.to_bits(),
            config.max_bank_height.to_bits(),
            config.fade.to_bits(),
        ]
    }

    #[test]
    fn river_configs_match_engine_0_3_0() {
        let levels = Levels::new(256, 63);
        let settings = RiverSettings::default();
        assert_eq!(settings.river_count, 8);
        assert_eq!(settings.main_rivers, River::new(5, 2, 6, 20, 8, 0.75));
        assert_eq!(settings.branch_rivers, River::new(4, 1, 4, 14, 5, 0.975));

        let main = RiverConfig::from_settings(settings.main_rivers, levels, true, 0);
        assert_eq!(
            (main.main, main.order, main.bed_width, main.bank_width),
            (true, 0, 8, 20)
        );
        assert_eq!(main.length2, 1_000_000);
        assert_eq!(
            river_bits(main),
            [0x3e640000, 0x3e800000, 0x3e880000, 0x3f400000]
        );

        let fork = main.create_fork(10.0, levels);
        assert_eq!(
            (fork.main, fork.order, fork.bed_width, fork.bank_width),
            (false, 1, 4, 10)
        );
        assert_eq!(
            river_bits(fork),
            [0x3e640000, 0x3e800000, 0x3e860000, 0x3f400000]
        );

        let lake = LakeConfig::new(settings.lakes, levels);
        assert_eq!(
            [
                lake.depth,
                lake.chance,
                lake.size_min,
                lake.size_max,
                lake.size_range,
                lake.bank_min,
                lake.bank_max,
                lake.distance_min,
                lake.distance_max
            ]
            .map(f32::to_bits),
            [
                0x3e500000, 0x3e99999a, 0x42960000, 0x43160000, 0x42960000, 0x3e800000, 0x3e900000,
                0, 0x3cf5c28f
            ]
        );

        let wetland = WetlandConfig::new(settings.wetlands);
        assert_eq!(wetland.skip_size, 4);
        assert_eq!(wetland.length.apply(0.5).to_bits(), 0x438fc000);
        assert_eq!(wetland.width.apply(0.5).to_bits(), 0x42fa0000);
        assert_eq!(Variance::min(0.25).apply(0.5).to_bits(), 0x3f200000);
        assert_eq!(
            Variance::range(0.25).apply_scaled(0.5, 2.0).to_bits(),
            0x3fe00000
        );
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
