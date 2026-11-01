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
    terraforged::{
        CurveFunction, Domain, Interpolation, Line, Module, Source, SourceConfig, SourceKind,
        cos_lookup, noise_map, simplex, sin_lookup,
    },
    terraforged_engine::{
        Cell, Lake, Levels, River as RiverSettings, Terrain, TerrainProperties, TerrainRegistry,
        Wetland,
    },
    terraforged_fancy::Vec2f,
    terraforged_util::{pack_floats, unpack_left_float, unpack_right_float},
};
use horizon_core::random::{LegacyRandom, RandomSource};
use std::time::{SystemTime, UNIX_EPOCH};

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

    pub fn next(self, random: &mut LegacyRandom) -> f32 {
        self.apply(random.next_float())
    }

    pub fn next_scaled(self, random: &mut LegacyRandom, scaler: f32) -> f32 {
        self.apply_scaled(random.next_float(), scaler)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Range {
    pub min: f32,
    pub max: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Boundsf {
    pub min_x: f32,
    pub min_y: f32,
    pub max_x: f32,
    pub max_y: f32,
}

impl Boundsf {
    pub const NONE: Self = Self::new(1.0, 1.0, -1.0, -1.0);

    pub const fn new(min_x: f32, min_y: f32, max_x: f32, max_y: f32) -> Self {
        Self {
            min_x,
            min_y,
            max_x,
            max_y,
        }
    }

    pub const fn contains(self, x: f32, y: f32) -> bool {
        x >= self.min_x && x <= self.max_x && y >= self.min_y && y <= self.max_y
    }

    pub const fn builder() -> BoundsfBuilder {
        BoundsfBuilder::new()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoundsfBuilder {
    min_x: f32,
    min_y: f32,
    max_x: f32,
    max_y: f32,
}

impl BoundsfBuilder {
    pub const fn new() -> Self {
        Self {
            min_x: f32::MAX,
            min_y: f32::MAX,
            max_x: f32::MIN_POSITIVE,
            max_y: f32::MIN_POSITIVE,
        }
    }

    pub fn record(&mut self, x: f32, y: f32) {
        self.min_x = self.min_x.min(x);
        self.min_y = self.min_y.min(y);
        self.max_x = self.max_x.max(x);
        self.max_y = self.max_y.max(y);
    }

    pub const fn build(self) -> Boundsf {
        Boundsf::new(self.min_x, self.min_y, self.max_x, self.max_y)
    }
}

impl Default for BoundsfBuilder {
    fn default() -> Self {
        Self::new()
    }
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
            noise_round(self.x1 + self.dx * factor) as f32,
            noise_round(self.z1 + self.dz * factor) as f32,
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
        dx += cos_lookup(radians) * river.norm_x * wiggle_distance;
        dz += sin_lookup(radians) * river.norm_z * wiggle_distance;
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
        noise_map(distance, 0.0, 0.075, 0.075)
    }
}

#[derive(Clone, Copy)]
pub struct RiverCarverSettings {
    pub valley_size: f32,
    pub fade_in: f32,
    pub connecting: bool,
    pub valley_curve: CurveFunction,
}

impl Default for RiverCarverSettings {
    fn default() -> Self {
        Self {
            valley_size: 275.0,
            fade_in: 0.7,
            connecting: false,
            valley_curve: CurveFunction::s_curve(2.0, -0.5),
        }
    }
}

pub struct RiverCarver {
    pub main: bool,
    _connecting: bool,
    fade: f32,
    fade_inv: f32,
    bed_width: Range,
    banks_width: Range,
    valley_width: Range,
    bed_depth: Range,
    banks_depth: Range,
    water_line: f32,
    river_terrain: Terrain,
    pub river: River,
    pub warp: RiverWarp,
    pub config: RiverConfig,
    pub valley_curve: CurveFunction,
}

impl RiverCarver {
    pub fn new(
        river: River,
        warp: RiverWarp,
        config: RiverConfig,
        settings: RiverCarverSettings,
        levels: Levels,
    ) -> Self {
        let valley_size2 = settings.valley_size * settings.valley_size;
        Self {
            main: config.main,
            _connecting: settings.connecting,
            fade: settings.fade_in,
            fade_inv: 1.0 / settings.fade_in,
            bed_width: Range::new(0.25, config.bed_width.wrapping_mul(config.bed_width) as f32),
            banks_width: Range::new(
                1.5625,
                config.bank_width.wrapping_mul(config.bank_width) as f32,
            ),
            valley_width: Range::new(valley_size2, valley_size2),
            bed_depth: Range::new(levels.water, config.bed_height),
            banks_depth: Range::new(config.min_bank_height, config.max_bank_height),
            water_line: levels.water,
            river_terrain: TerrainRegistry::new().get_by_id(5),
            river,
            warp,
            config,
            valley_curve: settings.valley_curve,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn carve(
        &self,
        cell: &mut Cell,
        previous_x: f32,
        previous_z: f32,
        previous_t: f32,
        x: f32,
        z: f32,
        t: f32,
    ) {
        let distance2 = self.distance2(x, z, t);
        let previous_distance2 = self.distance2(previous_x, previous_z, previous_t);
        let mut valley_alpha = self.distance_alpha(
            previous_t,
            distance2.min(previous_distance2),
            self.valley_width,
        );
        if valley_alpha == 0.0 {
            return;
        }
        let bank_height = self.scaled_size(t, self.banks_depth);
        valley_alpha = self.valley_curve.apply(valley_alpha);
        cell.river_mask = cell.river_mask.min(1.0 - valley_alpha);
        cell.value = lerp(cell.value, bank_height, valley_alpha).min(cell.value);

        let mouth_modifier = Self::mouth_modifier(cell);
        let bed_height = self.scaled_size(t, self.bed_depth);
        let banks_alpha = self.distance_alpha(t, distance2 * mouth_modifier, self.banks_width);
        if banks_alpha == 0.0 {
            return;
        }
        if cell.value > bed_height {
            cell.value = lerp(cell.value, bed_height, banks_alpha).min(cell.value);
            self.tag(cell, bed_height);
        }
        let bed_alpha = self.distance_alpha(t, distance2, self.bed_width);
        if bed_alpha != 0.0 && cell.value > bed_height {
            cell.value = lerp(cell.value, bed_height, bed_alpha);
            self.tag(cell, bed_height);
        }
    }

    pub fn create_fork_config(&self, t: f32, levels: Levels) -> RiverConfig {
        let bed_height = levels.scale_value(self.scaled_size(t, self.bed_depth));
        let bed_width =
            java_round_f64(f64::from(self.scaled_size(t, self.bed_width)).sqrt() * 0.75).max(1);
        let bank_width =
            java_round_f64(f64::from(self.scaled_size(t, self.banks_width)).sqrt() * 0.75)
                .max(bed_width + 1);
        self.config
            .create_fork_sized(bed_height, bed_width, bank_width, levels)
    }

    pub fn valley_type(random: &mut LegacyRandom) -> CurveFunction {
        let value = random.next_int_bound(100).expect("positive bound");
        if value < 5 {
            CurveFunction::s_curve(0.4, 1.0)
        } else if value < 30 {
            CurveFunction::s_curve(4.0, 5.0)
        } else if value < 50 {
            CurveFunction::s_curve(3.0, 0.25)
        } else {
            CurveFunction::s_curve(2.0, -0.5)
        }
    }

    pub fn create(
        x1: f32,
        z1: f32,
        x2: f32,
        z2: f32,
        config: RiverConfig,
        levels: Levels,
        random: &mut LegacyRandom,
    ) -> Self {
        let river = River::new(x1, z1, x2, z2);
        let warp = RiverWarp::create(0.35, random);
        let valley_size =
            River::VALLEY_WIDTH as f32 * River::MAIN_VALLEY.apply(random.next_float());
        let settings = RiverCarverSettings {
            valley_curve: Self::valley_type(random),
            fade_in: config.fade,
            valley_size,
            connecting: false,
        };
        Self::new(river, warp, config, settings, levels)
    }

    fn distance2(&self, x: f32, z: f32, t: f32) -> f32 {
        if t <= 0.0 {
            Line::dist2(x, z, self.river.x1, self.river.z1)
        } else if t >= 1.0 {
            Line::dist2(x, z, self.river.x2, self.river.z2)
        } else {
            Line::dist2(
                x,
                z,
                self.river.x1 + t * self.river.dx,
                self.river.z1 + t * self.river.dz,
            )
        }
    }

    fn distance_alpha(&self, t: f32, distance2: f32, range: Range) -> f32 {
        let size2 = self.scaled_size(t, range);
        if distance2 >= size2 {
            0.0
        } else {
            1.0 - distance2 / size2
        }
    }

    fn scaled_size(&self, t: f32, range: Range) -> f32 {
        if t < 0.0 {
            range.min
        } else if t > 1.0 || t >= self.fade {
            range.max
        } else if range.min == range.max {
            range.min
        } else {
            lerp(range.min, range.max, t * self.fade_inv)
        }
    }

    fn tag(&self, cell: &mut Cell, bed_height: f32) {
        if cell.terrain.overrides_river()
            && (cell.value < bed_height || cell.value > self.water_line)
        {
            return;
        }
        cell.erosion_mask = true;
        if cell.value <= self.water_line {
            cell.terrain = self.river_terrain.clone();
        }
    }

    fn mouth_modifier(cell: &Cell) -> f32 {
        let modifier = noise_map(cell.continent_edge, 0.0, 0.5, 0.5);
        modifier * modifier
    }
}

fn lerp(first: f32, second: f32, alpha: f32) -> f32 {
    first + alpha * (second - first)
}

fn java_round_f64(value: f64) -> i32 {
    (value + 0.5).floor() as i32
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

    pub fn from_settings(settings: RiverSettings, levels: Levels, main: bool, order: i32) -> Self {
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

pub struct LakeCarver {
    valley: f32,
    valley2: f32,
    lake_distance2: f32,
    valley_distance2: f32,
    bank_alpha_min: f32,
    bank_alpha_max: f32,
    bank_alpha_range: f32,
    depth: f32,
    bank_min: f32,
    bank_max: f32,
    center: Vec2f,
    lake_terrain: Terrain,
}

impl LakeCarver {
    pub fn new(center: Vec2f, radius: f32, multiplier: f32, config: LakeConfig) -> Self {
        let lake = radius * multiplier;
        let valley = 275.0 * multiplier;
        let valley2 = valley * valley;
        let lake_distance2 = lake * lake;
        let bank_alpha_min = config.bank_min;
        let bank_alpha_max = 1.0_f32.min(bank_alpha_min + 0.275);
        Self {
            valley,
            valley2,
            lake_distance2,
            valley_distance2: valley2 - lake_distance2,
            bank_alpha_min,
            bank_alpha_max,
            bank_alpha_range: bank_alpha_max - bank_alpha_min,
            depth: config.depth,
            bank_min: config.bank_min,
            bank_max: config.bank_max,
            center,
            lake_terrain: TerrainRegistry::new().get_by_id(6),
        }
    }

    pub fn apply(&self, cell: &mut Cell, x: f32, z: f32) {
        let distance2 = self.distance2(x, z);
        if distance2 > self.valley2 {
            return;
        }
        let bank_height = self.bank_height(cell);
        if distance2 > self.lake_distance2 {
            if cell.value < bank_height {
                return;
            }
            let valley_alpha = clamp(
                1.0 - (distance2 - self.lake_distance2) / self.valley_distance2,
                0.0,
                1.0,
            );
            cell.value = lerp(cell.value, bank_height, valley_alpha);
            cell.river_mask *= 1.0 - valley_alpha;
            cell.river_mask = cell.river_mask.min(1.0 - valley_alpha);
            return;
        }
        cell.value = bank_height.min(cell.value);
        if distance2 < self.lake_distance2 {
            let depth_alpha = clamp(1.0 - distance2 / self.lake_distance2, 0.0, 1.0);
            let lake_depth = cell.value.min(self.depth);
            cell.value = lerp(cell.value, lake_depth, depth_alpha);
            cell.terrain = self.lake_terrain.clone();
            cell.river_mask = cell.river_mask.min(1.0 - depth_alpha);
        }
    }

    pub fn record_bounds(&self, builder: &mut BoundsfBuilder) {
        let radius = self.valley * 1.2;
        builder.record(self.center.x - radius, self.center.y - radius);
        builder.record(self.center.x + radius, self.center.y + radius);
    }

    pub fn overlaps(&self, x: f32, z: f32, radius2: f32) -> bool {
        self.distance2(x, z) < self.lake_distance2 + radius2
    }

    fn distance2(&self, x: f32, z: f32) -> f32 {
        let dx = self.center.x - x;
        let dz = self.center.y - z;
        dx * dx + dz * dz
    }

    fn bank_height(&self, cell: &Cell) -> f32 {
        let alpha = noise_map(
            cell.value,
            self.bank_alpha_min,
            self.bank_alpha_max,
            self.bank_alpha_range,
        );
        lerp(self.bank_min, self.bank_max, alpha)
    }
}

pub struct WetlandCarver {
    start: Vec2f,
    end: Vec2f,
    radius: f32,
    radius2: f32,
    bed: f32,
    banks: f32,
    mound_min: f32,
    mound_max: f32,
    mound_variance: f32,
    mound_shape: Module,
    mound_height: Module,
    terrain_edge: Module,
    wetland_terrain: Terrain,
}

impl WetlandCarver {
    pub fn new(mut seed: i32, start: Vec2f, end: Vec2f, radius: f32, levels: Levels) -> Self {
        let bed = levels.water(-1) - 0.5 / levels.world_height as f32;
        let banks = levels.ground(3);
        let mound_min = levels.water(1);
        let mound_max = levels.water(2);
        seed = seed.wrapping_add(1);
        let mound_shape = wetland_source(SourceKind::Perlin, seed, 10)
            .clamp(Module::constant(0.3), Module::constant(0.6))
            .map(Module::constant(0.0), Module::constant(1.0));
        seed = seed.wrapping_add(1);
        let mound_height = wetland_source(SourceKind::Simplex, seed, 20)
            .clamp(Module::constant(0.0), Module::constant(0.3))
            .map(Module::constant(0.0), Module::constant(1.0));
        seed = seed.wrapping_add(1);
        let terrain_edge = wetland_source(SourceKind::Perlin, seed, 8)
            .clamp(Module::constant(0.2), Module::constant(0.8))
            .map(Module::constant(0.0), Module::constant(0.9));
        Self {
            start,
            end,
            radius,
            radius2: radius * radius,
            bed,
            banks,
            mound_min,
            mound_max,
            mound_variance: mound_max - mound_min,
            mound_shape,
            mound_height,
            terrain_edge,
            wetland_terrain: TerrainRegistry::new().get_by_id(7),
        }
    }

    pub fn apply(&self, cell: &mut Cell, x: f32, z: f32) {
        self.apply_warped(cell, x, z, x, z);
    }

    pub fn apply_warped(&self, cell: &mut Cell, river_x: f32, river_z: f32, x: f32, z: f32) {
        if cell.value < self.bed {
            return;
        }
        let t = Line::distance_on_line(
            river_x,
            river_z,
            self.start.x,
            self.start.y,
            self.end.x,
            self.end.y,
        );
        let distance2 = line_distance2(
            river_x,
            river_z,
            self.start.x,
            self.start.y,
            self.end.x,
            self.end.y,
            t,
        );
        if distance2 > self.radius2 {
            return;
        }
        let distance = 1.0 - distance2 / self.radius2;
        if distance <= 0.0 {
            return;
        }
        let valley_alpha = noise_map(distance, 0.0, 0.65, 0.65);
        if cell.value > self.banks {
            cell.value = lerp(cell.value, self.banks, valley_alpha);
        }
        let pools_alpha = noise_map(distance, 0.65, 0.7, 0.050000012);
        if cell.value > self.bed && cell.value <= self.banks {
            cell.value = lerp(cell.value, self.bed, pools_alpha);
        }
        if pools_alpha >= 1.0 {
            cell.erosion_mask = true;
        }
        if distance > 0.65 && pools_alpha > self.terrain_edge.value(x, z) {
            cell.terrain = self.wetland_terrain.clone();
        }
        if cell.value >= self.bed && cell.value < self.mound_max {
            let shape_alpha = self.mound_shape.value(x, z) * pools_alpha;
            let mounds = self.mound_min + self.mound_height.value(x, z) * self.mound_variance;
            cell.value = lerp(cell.value, mounds, shape_alpha);
        }
        cell.river_mask = cell.river_mask.min(1.0 - valley_alpha);
    }

    pub fn record_bounds(&self, builder: &mut BoundsfBuilder) {
        builder.record(
            self.start.x.min(self.end.x) - self.radius,
            self.start.y.min(self.end.y) - self.radius,
        );
        builder.record(
            self.start.x.max(self.end.x) + self.radius,
            self.start.y.max(self.end.y) + self.radius,
        );
    }
}

pub struct RiverNetwork {
    bounds: Boundsf,
    lakes: Vec<LakeCarver>,
    wetlands: Vec<WetlandCarver>,
    children: Vec<Self>,
    river_carver: RiverCarver,
}

impl RiverNetwork {
    pub fn new(
        river_carver: RiverCarver,
        lakes: Vec<LakeCarver>,
        wetlands: Vec<WetlandCarver>,
        children: Vec<Self>,
        bounds: Boundsf,
    ) -> Self {
        Self {
            bounds,
            lakes,
            wetlands,
            children,
            river_carver,
        }
    }

    pub const fn contains(&self, x: f32, z: f32) -> bool {
        self.bounds.contains(x, z)
    }

    pub fn carve(&self, cell: &mut Cell, mut x: f32, mut z: f32, lake_x: f32, lake_z: f32) {
        let river = self.river_carver.river;
        let warp = self.river_carver.warp;
        let mut t = Line::distance_on_line(x, z, river.x1, river.z1, river.x2, river.z2);
        let previous_x = x;
        let previous_z = z;
        let previous_t = t;
        if warp.test(t) {
            let offset = warp.offset(x, z, previous_t, river);
            x += unpack_left_float(offset);
            z += unpack_right_float(offset);
            t = Line::distance_on_line(x, z, river.x1, river.z1, river.x2, river.z2);
        }
        self.river_carver
            .carve(cell, previous_x, previous_z, previous_t, x, z, t);
        for wetland in &self.wetlands {
            wetland.apply_warped(cell, x + lake_x, z + lake_z, x, z);
        }
        for lake in &self.lakes {
            lake.apply(cell, x + lake_x, z + lake_z);
        }
        for child in &self.children {
            child.carve(cell, x, z, lake_x, lake_z);
        }
    }

    pub fn overlaps(&self, river: River, extend: f32) -> bool {
        self.river_carver.river.intersects_extended(river, extend)
            || self
                .children
                .iter()
                .any(|child| child.overlaps(river, extend))
    }

    pub fn builder(carver: RiverCarver) -> RiverNetworkBuilder {
        RiverNetworkBuilder::new(carver)
    }
}

pub struct RiverNetworkBuilder {
    pub carver: RiverCarver,
    pub lakes: Vec<LakeCarver>,
    pub wetlands: Vec<WetlandCarver>,
    pub children: Vec<Self>,
    _min_x: f32,
    _min_z: f32,
    _max_x: f32,
    _max_z: f32,
}

impl RiverNetworkBuilder {
    pub fn new(carver: RiverCarver) -> Self {
        let mut builder = Self {
            carver,
            lakes: Vec::new(),
            wetlands: Vec::new(),
            children: Vec::new(),
            _min_x: 0.0,
            _min_z: 0.0,
            _max_x: 0.0,
            _max_z: 0.0,
        };
        let river = builder.carver.river;
        builder.add_bounds(river);
        builder
    }

    pub fn add_bounds(&mut self, river: River) {
        self._min_x = self._min_x.min(river.x1).min(river.x2);
        self._min_z = self._min_z.min(river.z1).min(river.z2);
        self._max_x = self._max_x.max(river.x1).max(river.x2);
        self._max_z = self._max_z.max(river.z1).max(river.z2);
    }

    pub fn overlaps(
        &self,
        river: River,
        parent: Option<&RiverNetworkBuilder>,
        extend: f32,
    ) -> bool {
        let parent_is_self = parent.is_some_and(|parent| std::ptr::eq(parent, self));
        if parent_is_self {
            let x1 = river.x1 - river.ndx * extend;
            let z1 = river.z1 - river.ndz * extend;
            let x2 = river.x1 + river.dx * 0.5;
            let z2 = river.z1 + river.dz * 0.5;
            let other = self.carver.river;
            if Line::intersect(x1, z1, x2, z2, other.x1, other.z1, other.x2, other.z2) {
                return true;
            }
        } else if self.carver.river.intersects_extended(river, extend) {
            return true;
        }
        if parent.is_some()
            && !parent_is_self
            && self.carver.river.intersects_extended(river, extend)
        {
            return true;
        }
        self.children
            .iter()
            .any(|branch| branch.overlaps(river, parent, extend))
    }

    pub fn build(self) -> RiverNetwork {
        let mut builder = Boundsf::builder();
        self.record_bounds(&mut builder);
        self.build_with_bounds(builder.build())
    }

    fn build_with_bounds(self, bounds: Boundsf) -> RiverNetwork {
        let children = self
            .children
            .into_iter()
            .map(|child| child.build_with_bounds(Boundsf::NONE))
            .collect();
        RiverNetwork::new(self.carver, self.lakes, self.wetlands, children, bounds)
    }

    fn record_bounds(&self, builder: &mut BoundsfBuilder) {
        let river = self.carver.river;
        builder.record(river.min_x, river.min_z);
        builder.record(river.max_x, river.max_z);
        for child in &self.children {
            child.record_bounds(builder);
        }
        for lake in &self.lakes {
            lake.record_bounds(builder);
        }
        for wetland in &self.wetlands {
            wetland.record_bounds(builder);
        }
    }
}

pub struct Rivermap {
    x: i32,
    z: i32,
    lake_warp: Domain,
    river_warp: Domain,
    networks: Vec<RiverNetwork>,
    timestamp: u128,
}

impl Rivermap {
    pub fn new(x: i32, z: i32, networks: Vec<RiverNetwork>, warp: GenWarp) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        Self {
            x,
            z,
            lake_warp: warp.lake,
            river_warp: warp.river,
            networks,
            timestamp,
        }
    }

    pub fn apply(&self, cell: &mut Cell, x: f32, z: f32) {
        let river_x = self.river_warp.x(x, z);
        let river_z = self.river_warp.y(x, z);
        let lake_x = self.lake_warp.offset_x(river_x, river_z);
        let lake_z = self.lake_warp.offset_y(river_x, river_z);
        for network in &self.networks {
            if network.contains(river_x, river_z) {
                network.carve(cell, river_x, river_z, lake_x, lake_z);
            }
        }
    }

    pub const fn timestamp(&self) -> u128 {
        self.timestamp
    }

    pub const fn x(&self) -> i32 {
        self.x
    }

    pub const fn z(&self) -> i32 {
        self.z
    }
}

pub trait ContinentSampler {
    fn edge_value(&self, x: f32, z: f32) -> f32;
}

impl ContinentSampler for super::terraforged_fancy::FancyContinentGenerator {
    fn edge_value(&self, x: f32, z: f32) -> f32 {
        super::terraforged_fancy::FancyContinentGenerator::edge_value(self, x, z)
    }
}

pub struct BaseRiverGenerator<C> {
    pub count: i32,
    pub continent_scale: i32,
    pub min_edge_value: f32,
    pub seed: i32,
    pub lake: LakeConfig,
    pub main: RiverConfig,
    pub fork: RiverConfig,
    pub wetland: WetlandConfig,
    pub continent: C,
    pub levels: Levels,
}

impl<C: ContinentSampler> BaseRiverGenerator<C> {
    pub fn new(
        continent: C,
        root_seed: i32,
        world: super::terraforged_engine::WorldSettings,
        rivers: super::terraforged_engine::RiverSettings,
        levels: Levels,
    ) -> Self {
        let main = RiverConfig::builder(levels)
            .bank_height(
                rivers.main_rivers.min_bank_height,
                rivers.main_rivers.max_bank_height,
            )
            .bank_width(rivers.main_rivers.bank_width)
            .bed_width(rivers.main_rivers.bed_width)
            .bed_depth(rivers.main_rivers.bed_depth)
            .fade(f64::from(rivers.main_rivers.fade))
            .length(5000)
            .main(true)
            .order(0)
            .build();
        let fork = RiverConfig::builder(levels)
            .bank_height(
                rivers.branch_rivers.min_bank_height,
                rivers.branch_rivers.max_bank_height,
            )
            .bank_width(rivers.branch_rivers.bank_width)
            .bed_width(rivers.branch_rivers.bed_width)
            .bed_depth(rivers.branch_rivers.bed_depth)
            .fade(f64::from(rivers.branch_rivers.fade))
            .length(4500)
            .order(1)
            .build();
        Self {
            count: rivers.river_count,
            continent_scale: world.continent.continent_scale,
            min_edge_value: world.control_points.inland,
            seed: root_seed.wrapping_add(rivers.seed_offset),
            lake: LakeConfig::new(rivers.lakes, levels),
            main,
            fork,
            wetland: WetlandConfig::new(rivers.wetlands),
            continent,
            levels,
        }
    }

    pub fn generate_rivers(
        &self,
        x: i32,
        z: i32,
        id: i64,
        roots: impl FnOnce(&Self, i32, i32, &mut LegacyRandom, &GenWarp) -> Vec<RiverNetworkBuilder>,
    ) -> Rivermap {
        let mut random = LegacyRandom::new(id.wrapping_add(i64::from(self.seed)));
        let warp = GenWarp::new(id as i32, self.continent_scale);
        let mut rivers = roots(self, x, z, &mut random, &warp);
        java_shuffle(&mut rivers, &mut random);
        let mut occupied: Vec<River> = rivers.iter().map(|root| root.carver.river).collect();
        for root in &mut rivers {
            self.generate_forks(
                root,
                River::MAIN_SPACING,
                self.fork,
                &mut random,
                &warp,
                &mut occupied,
                0,
            );
        }
        for river in &mut rivers {
            self.generate_wetlands(river, &mut random);
        }
        Rivermap::new(
            x,
            z,
            rivers.into_iter().map(RiverNetworkBuilder::build).collect(),
            warp,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn generate_forks(
        &self,
        parent: &mut RiverNetworkBuilder,
        spacing: Variance,
        config: RiverConfig,
        random: &mut LegacyRandom,
        warp: &GenWarp,
        occupied: &mut Vec<River>,
        depth: i32,
    ) {
        if depth > 2 {
            return;
        }
        let length = 0.44 * parent.carver.river.length;
        if length < 300.0 {
            return;
        }
        let mut direction = if random.next_boolean() { 1 } else { -1 };
        let mut offset = 0.25;
        while offset < 0.9 {
            direction = -direction;
            let parent_angle = parent.carver.river.angle();
            let fork_angle = direction as f32 * java_pi2() * River::FORK_ANGLE.next(random);
            let angle = parent_angle + fork_angle;
            let dx = sin_lookup(angle);
            let dz = cos_lookup(angle);
            let start = parent.carver.river.pos(offset);
            let x1 = unpack_left_float(start);
            let z1 = unpack_right_float(start);
            let x2 = x1 - dx * length;
            let z2 = z1 - dz * length;
            if self.continent.edge_value(x1, z1) >= self.min_edge_value
                && self.continent.edge_value(x2, z2) >= self.min_edge_value
            {
                let fork_config = parent.carver.create_fork_config(offset, self.levels);
                let river = River::new(x2, z2, x1, z1);
                if !river_overlaps_geometry(river, parent.carver.river, occupied) {
                    let settings = RiverCarverSettings {
                        connecting: true,
                        fade_in: config.fade,
                        valley_size: River::VALLEY_WIDTH as f32 * River::FORK_VALLEY.next(random),
                        valley_curve: RiverCarver::valley_type(random),
                    };
                    let fork_warp = parent.carver.warp.child(0.15, 0.75, 0.65, random);
                    let carver =
                        RiverCarver::new(river, fork_warp, fork_config, settings, self.levels);
                    occupied.push(river);
                    let mut builder = RiverNetwork::builder(carver);
                    self.generate_forks(
                        &mut builder,
                        River::FORK_SPACING,
                        config,
                        random,
                        warp,
                        occupied,
                        depth + 1,
                    );
                    parent.children.push(builder);
                }
            }
            offset += spacing.next(random);
        }
        self.add_lake(parent, random, warp);
    }

    pub fn generate_wetlands(&self, builder: &mut RiverNetworkBuilder, random: &mut LegacyRandom) {
        let skip = random
            .next_int_bound(self.wetland.skip_size)
            .expect("positive wetland skip size");
        if skip == 0 {
            let width = self.wetland.width.next(random);
            let length = self.wetland.length.next(random);
            let river_length = builder.carver.river.length();
            let start_pos = random.next_float() * 0.75;
            let end_pos = start_pos + random.next_float() * (length / river_length);
            let start = builder.carver.river.pos(start_pos);
            let end = builder.carver.river.pos(end_pos);
            builder.wetlands.push(WetlandCarver::new(
                random.next_int(),
                Vec2f::new(unpack_left_float(start), unpack_right_float(start)),
                Vec2f::new(unpack_left_float(end), unpack_right_float(end)),
                width,
                self.levels,
            ));
        }
        for child in &mut builder.children {
            self.generate_wetlands(child, random);
        }
    }

    pub fn add_lake(
        &self,
        branch: &mut RiverNetworkBuilder,
        random: &mut LegacyRandom,
        _warp: &GenWarp,
    ) {
        if random.next_float() <= self.lake.chance {
            let center_x = branch.carver.river.x1;
            let center_z = branch.carver.river.z1;
            let lake_size = self.lake.size_min + random.next_float() * self.lake.size_range;
            if branch
                .lakes
                .iter()
                .any(|lake| lake.overlaps(center_x, center_z, lake_size * lake_size))
            {
                return;
            }
            branch.lakes.push(LakeCarver::new(
                Vec2f::new(center_x, center_z),
                lake_size,
                1.0,
                self.lake,
            ));
        }
    }

    pub fn generate_additional_lakes(
        &self,
        x: i32,
        z: i32,
        random: &mut LegacyRandom,
        roots: &[RiverNetworkBuilder],
        rivers: &[RiverCarver],
        lakes: &mut Vec<LakeCarver>,
    ) {
        let size = 150.0;
        let size_variance = Variance::of(1.0, 0.25);
        let _angle_variance = Variance::of(1.99, 0.02);
        let distance_variance = Variance::of(0.6, 0.3);
        for index in 1..roots.len() {
            let first = &roots[index - 1];
            let _second = &roots[index];
            let angle = 0.0;
            let dx = sin_lookup(angle);
            let dz = cos_lookup(angle);
            let distance = distance_variance.next(random);
            let lake_x = x as f32 + dx * first.carver.river.length * distance;
            let lake_z = z as f32 + dz * first.carver.river.length * distance;
            let variance = size_variance.next(random);
            let center = Vec2f::new(lake_x, lake_z);
            if rivers
                .iter()
                .any(|river| !river.main && river.river.overlaps_circle(center, size))
            {
                continue;
            }
            lakes.push(LakeCarver::new(center, size, variance, self.lake));
        }
    }

    pub fn create_main(
        &self,
        x1: f32,
        z1: f32,
        x2: f32,
        z2: f32,
        random: &mut LegacyRandom,
    ) -> RiverCarver {
        RiverCarver::create(x1, z1, x2, z2, self.main, self.levels, random)
    }

    pub fn create_fork(
        &self,
        x1: f32,
        z1: f32,
        x2: f32,
        z2: f32,
        valley_size: f32,
        config: RiverConfig,
        random: &mut LegacyRandom,
    ) -> RiverCarver {
        let warp = RiverWarp::create(0.4, random);
        let settings = RiverCarverSettings {
            connecting: true,
            fade_in: config.fade,
            valley_size,
            valley_curve: RiverCarver::valley_type(random),
        };
        RiverCarver::new(
            River::new(x1, z1, x2, z2),
            warp,
            config,
            settings,
            self.levels,
        )
    }
}

fn java_shuffle<T>(values: &mut [T], random: &mut LegacyRandom) {
    for index in (1..values.len()).rev() {
        let swap = random
            .next_int_bound((index + 1) as i32)
            .expect("positive shuffle bound") as usize;
        values.swap(index, swap);
    }
}

fn river_overlaps_geometry(candidate: River, parent: River, occupied: &[River]) -> bool {
    let x1 = candidate.x1 - candidate.ndx * 250.0;
    let z1 = candidate.z1 - candidate.ndz * 250.0;
    let x2 = candidate.x1 + candidate.dx * 0.5;
    let z2 = candidate.z1 + candidate.dz * 0.5;
    if Line::intersect(x1, z1, x2, z2, parent.x1, parent.z1, parent.x2, parent.z2) {
        return true;
    }
    occupied
        .iter()
        .filter(|river| **river != parent)
        .any(|river| river.intersects_extended(candidate, 250.0))
}

#[allow(clippy::too_many_arguments)]
fn line_distance2(x: f32, y: f32, ax: f32, ay: f32, bx: f32, by: f32, t: f32) -> f32 {
    if t <= 0.0 {
        Line::dist2(x, y, ax, ay)
    } else if t >= 1.0 {
        Line::dist2(x, y, bx, by)
    } else {
        Line::dist2(x, y, ax + t * (bx - ax), ay + t * (by - ay))
    }
}

fn wetland_source(kind: SourceKind, seed: i32, scale: i32) -> Module {
    Module::source(Source::new(
        kind,
        SourceConfig {
            seed,
            octaves: 1,
            gain: 0.5,
            frequency: 1.0 / scale as f32,
            lacunarity: 2.0,
            interpolation: Interpolation::Curve3,
        },
    ))
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
    if value >= 0.0 {
        (value + 0.5) as i32
    } else {
        (value - 0.5) as i32
    }
}

fn clamp(value: f32, min: f32, max: f32) -> f32 {
    value.max(min).min(max)
}

fn java_pi2() -> f32 {
    f32::from_bits(0x40c9_0fdb)
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
    use super::super::terraforged_engine::{RiverSettings as DefaultRiverSettings, WorldSettings};
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
        let settings = DefaultRiverSettings::default();
        assert_eq!(settings.river_count, 8);
        assert_eq!(
            settings.main_rivers,
            RiverSettings::new(5, 2, 6, 20, 8, 0.75)
        );
        assert_eq!(
            settings.branch_rivers,
            RiverSettings::new(4, 1, 4, 14, 5, 0.975)
        );

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

    #[test]
    fn river_carver_matches_engine_0_3_0() {
        let levels = Levels::new(256, 63);
        let config = RiverConfig::builder(levels)
            .main(true)
            .bed_width(8)
            .bank_width(20)
            .bed_depth(5)
            .bank_height(2, 6)
            .fade(0.75)
            .length(1000)
            .build();
        let carver = RiverCarver::new(
            River::new(0.0, 0.0, 1000.0, 0.0),
            RiverWarp::none(),
            config,
            RiverCarverSettings::default(),
            levels,
        );
        let sample = |previous_x, previous_z, previous_t, x, z, t| {
            let mut cell = Cell::default();
            cell.value = 0.6;
            cell.river_mask = 1.0;
            cell.continent_edge = 0.4;
            carver.carve(&mut cell, previous_x, previous_z, previous_t, x, z, t);
            (
                cell.value.to_bits(),
                cell.river_mask.to_bits(),
                cell.erosion_mask,
                cell.terrain.id(),
            )
        };
        assert_eq!(
            [
                sample(450.0, 0.0, 0.45, 500.0, 0.0, 0.5),
                sample(450.0, 10.0, 0.45, 500.0, 10.0, 0.5),
                sample(450.0, 100.0, 0.45, 500.0, 100.0, 0.5),
                sample(450.0, 300.0, 0.45, 500.0, 300.0, 0.5),
                sample(40.0, 5.0, 0.04, 50.0, 5.0, 0.05),
                sample(1050.0, 0.0, 1.05, 1100.0, 0.0, 1.1),
            ],
            [
                (0x3e69b6db, 0, true, 5),
                (0x3e71688f, 0x3b020100, true, 5),
                (0x3ea84507, 0x3e4bf690, false, 0),
                (0x3f19999a, 0x3f800000, false, 0),
                (0x3e7c4cbc, 0x3a01fc00, true, 0),
                (0x3e908257, 0x3d4b9540, false, 0),
            ]
        );

        assert_eq!(
            [-0.1, 0.0, 0.1, 0.5, 0.75, 1.0, 1.1].map(|t| {
                let fork = carver.create_fork_config(t, levels);
                (
                    fork.bed_width,
                    fork.bank_width,
                    fork.bed_height.to_bits(),
                    fork.min_bank_height.to_bits(),
                    fork.max_bank_height.to_bits(),
                )
            }),
            [
                (1, 2, 0x3e780000, 0x3e800000, 0x3e860000),
                (1, 2, 0x3e780000, 0x3e800000, 0x3e860000),
                (2, 6, 0x3e740000, 0x3e800000, 0x3e860000),
                (5, 13, 0x3e680000, 0x3e800000, 0x3e860000),
                (6, 15, 0x3e640000, 0x3e800000, 0x3e860000),
                (6, 15, 0x3e640000, 0x3e800000, 0x3e860000),
                (6, 15, 0x3e640000, 0x3e800000, 0x3e860000),
            ]
        );
        assert_eq!(
            [0, 1, 2, 3, 4, 12_345].map(|seed| {
                RiverCarver::valley_type(&mut LegacyRandom::new(seed))
                    .apply(0.5)
                    .to_bits()
            }),
            [
                0x3e9837f0, 0x3e9837f0, 0x3c3504f3, 0x3deac0c7, 0x3e9837f0, 0x3e9837f0
            ]
        );
    }

    #[test]
    fn lake_carver_matches_engine_0_3_0() {
        let levels = Levels::new(256, 63);
        let config = LakeConfig::new(DefaultRiverSettings::default().lakes, levels);
        let lake = LakeCarver::new(Vec2f::new(100.0, -50.0), 75.0, 0.8, config);
        assert_eq!(
            [100.0, 130.0, 160.0, 175.0, 200.0, 300.0, 321.0].map(|x| {
                let mut cell = Cell::default();
                cell.value = 0.6;
                cell.river_mask = 1.0;
                lake.apply(&mut cell, x, -50.0);
                (
                    cell.value.to_bits(),
                    cell.river_mask.to_bits(),
                    cell.terrain.id(),
                )
            }),
            [
                (0x3e500000, 0, 6),
                (0x3e640000, 0x3e800000, 6),
                (0x3e900000, 0x3f800000, 0),
                (0x3e976075, 0x3d392490, 0),
                (0x3ea75075, 0x3e124924, 0),
                (0x3f0a4ccd, 0x3f500000, 0),
                (0x3f19999a, 0x3f800000, 0),
            ]
        );
        let mut builder = Boundsf::builder();
        lake.record_bounds(&mut builder);
        let bounds = builder.build();
        assert_eq!(
            [bounds.min_x, bounds.min_y, bounds.max_x, bounds.max_y].map(f32::to_bits),
            [0xc3240000, 0xc39d0000, 0x43b60000, 0x43560000]
        );
        assert_eq!(
            [100.0, 160.0, 175.0, 200.0, 300.0].map(|x| lake.overlaps(x, -50.0, 400.0)),
            [true, true, false, false, false]
        );
        assert!(bounds.contains(100.0, -50.0));
        assert!(!Boundsf::NONE.contains(0.0, 0.0));
    }

    #[test]
    fn wetland_carver_matches_engine_0_3_0() {
        let wetland = WetlandCarver::new(
            12_345,
            Vec2f::new(0.0, 0.0),
            Vec2f::new(500.0, 200.0),
            100.0,
            Levels::new(256, 63),
        );
        let sample = |river_x, river_z, x, z, value| {
            let mut cell = Cell::default();
            cell.value = value;
            cell.river_mask = 1.0;
            wetland.apply_warped(&mut cell, river_x, river_z, x, z);
            (
                cell.value.to_bits(),
                cell.river_mask.to_bits(),
                cell.erosion_mask,
                cell.terrain.id(),
            )
        };
        assert_eq!(
            [
                sample(0.0, 0.0, 0.0, 0.0, 0.6),
                sample(250.0, 100.0, 250.0, 100.0, 0.6),
                sample(250.0, 150.0, 250.0, 150.0, 0.6),
                sample(250.0, 190.0, 250.0, 190.0, 0.6),
                sample(250.0, 210.0, 250.0, 210.0, 0.6),
                sample(550.0, 220.0, 550.0, 220.0, 0.6),
                sample(250.0, 100.0, 1000.0, -1000.0, 0.6),
                sample(250.0, 100.0, 250.0, 100.0, 0.1),
            ],
            [
                (0x3e7b5555, 0, true, 7),
                (0x3e7b5555, 0, true, 7),
                (0x3e7b5555, 0, true, 7),
                (0x3ee1dfaf, 0x3f092ac9, false, 0),
                (0x3f19999a, 0x3f800000, false, 0),
                (0x3e7b5555, 0, true, 7),
                (0x3e7b5555, 0, true, 7),
                (0x3dcccccd, 0x3f800000, false, 0),
            ]
        );
        let mut builder = Boundsf::builder();
        wetland.record_bounds(&mut builder);
        let bounds = builder.build();
        assert_eq!(
            [bounds.min_x, bounds.min_y, bounds.max_x, bounds.max_y].map(f32::to_bits),
            [0xc2c80000, 0xc2c80000, 0x44160000, 0x43960000]
        );
    }

    #[test]
    fn river_network_matches_engine_0_3_0() {
        let levels = Levels::new(256, 63);
        let make_carver = |x1, z1, x2, z2, main| {
            let config = RiverConfig::builder(levels)
                .main(main)
                .bed_width(8)
                .bank_width(20)
                .bed_depth(5)
                .bank_height(2, 6)
                .fade(0.75)
                .build();
            RiverCarver::new(
                River::new(x1, z1, x2, z2),
                RiverWarp::none(),
                config,
                RiverCarverSettings::default(),
                levels,
            )
        };
        let mut root = RiverNetwork::builder(make_carver(0.0, 0.0, 1000.0, 0.0, true));
        root.lakes.push(LakeCarver::new(
            Vec2f::new(200.0, 100.0),
            75.0,
            0.8,
            LakeConfig::new(DefaultRiverSettings::default().lakes, levels),
        ));
        root.wetlands.push(WetlandCarver::new(
            12_345,
            Vec2f::new(600.0, -100.0),
            Vec2f::new(900.0, -100.0),
            80.0,
            levels,
        ));
        root.children.push(RiverNetwork::builder(make_carver(
            500.0, -500.0, 500.0, 500.0, false,
        )));
        let network = root.build();
        assert_eq!(
            [
                (0.0, 0.0),
                (500.0, 500.0),
                (1000.0, 1000.0),
                (200.0, 100.0),
                (750.0, -100.0)
            ]
            .map(|(x, z)| network.contains(x, z)),
            [true, true, true, true, true]
        );
        assert_eq!(
            [
                (500.0, 0.0, 0.0, 0.0),
                (200.0, 100.0, 0.0, 0.0),
                (750.0, -100.0, 0.0, 0.0),
                (500.0, 100.0, 20.0, -10.0),
                (1100.0, 0.0, 0.0, 0.0)
            ]
            .map(|(x, z, lake_x, lake_z)| {
                let mut cell = Cell::default();
                cell.value = 0.6;
                cell.river_mask = 1.0;
                cell.continent_edge = 0.4;
                network.carve(&mut cell, x, z, lake_x, lake_z);
                (
                    cell.value.to_bits(),
                    cell.river_mask.to_bits(),
                    cell.erosion_mask,
                    cell.terrain.id(),
                )
            }),
            [
                (0x3e69b6db, 0, true, 5),
                (0x3e500000, 0, false, 6),
                (0x3e7b5555, 0, true, 7),
                (0x3e66db6e, 0, true, 5),
                (0x3eaa199f, 0x3e4bf690, false, 0),
            ]
        );
        assert_eq!(
            [
                River::new(500.0, -100.0, 500.0, 100.0),
                River::new(2000.0, 2000.0, 2100.0, 2100.0),
                River::new(-100.0, 0.0, 100.0, 0.0)
            ]
            .map(|river| network.overlaps(river, 50.0)),
            [true, false, true]
        );
    }

    #[test]
    fn rivermap_matches_engine_0_3_0() {
        let levels = Levels::new(256, 63);
        let config = RiverConfig::builder(levels)
            .main(true)
            .bed_width(8)
            .bank_width(20)
            .bed_depth(5)
            .bank_height(2, 6)
            .fade(0.75)
            .build();
        let carver = RiverCarver::new(
            River::new(0.0, 0.0, 1000.0, 0.0),
            RiverWarp::none(),
            config,
            RiverCarverSettings::default(),
            levels,
        );
        let map = Rivermap::new(
            17,
            -9,
            vec![RiverNetwork::builder(carver).build()],
            GenWarp::new(12_345, 3_000),
        );
        assert_eq!((map.x(), map.z()), (17, -9));
        assert!(map.timestamp() > 0);
        assert_eq!(
            [
                (0.0, 0.0),
                (500.0, 0.0),
                (500.0, 50.0),
                (1000.0, 0.0),
                (2000.0, 2000.0)
            ]
            .map(|(x, z)| {
                let mut cell = Cell::default();
                cell.value = 0.6;
                cell.river_mask = 1.0;
                cell.continent_edge = 0.4;
                map.apply(&mut cell, x, z);
                (
                    cell.value.to_bits(),
                    cell.river_mask.to_bits(),
                    cell.erosion_mask,
                    cell.terrain.id(),
                )
            }),
            [
                (0x3e780000, 0, true, 5),
                (0x3e69b24e, 0x38f84000, true, 5),
                (0x3e8e76a0, 0x3d4ca9c0, false, 0),
                (0x3e6688dc, 0x3a78b000, true, 5),
                (0x3f19999a, 0x3f800000, false, 0),
            ]
        );
    }

    #[test]
    fn base_river_generator_matches_engine_0_3_0() {
        struct AlwaysLand;
        impl ContinentSampler for AlwaysLand {
            fn edge_value(&self, _x: f32, _z: f32) -> f32 {
                1.0
            }
        }

        let levels = Levels::new(256, 63);
        let world = WorldSettings {
            seed: 12_345,
            ..WorldSettings::default()
        };
        let rivers = DefaultRiverSettings {
            seed_offset: 77,
            ..DefaultRiverSettings::default()
        };
        let generator = BaseRiverGenerator::new(AlwaysLand, 12_345, world, rivers, levels);
        assert_eq!(
            (
                generator.count,
                generator.continent_scale,
                generator.min_edge_value.to_bits(),
                generator.seed,
                generator.main.length,
                generator.fork.length,
                generator.wetland.skip_size,
            ),
            (8, 3000, 0x3f008312, 12_422, 5000, 4500, 4)
        );
        let map =
            generator.generate_rivers(17, -9, 0x1234_5678, |generator, _x, _z, random, _warp| {
                vec![RiverNetwork::builder(
                    generator.create_main(0.0, 0.0, 2500.0, 0.0, random),
                )]
            });
        assert_eq!(
            [
                (0.0, 0.0),
                (500.0, 0.0),
                (1000.0, 0.0),
                (1500.0, 0.0),
                (2000.0, 0.0),
                (2500.0, 0.0),
                (1250.0, 200.0),
                (500.0, -500.0),
                (3000.0, 3000.0),
            ]
            .map(|(x, z)| {
                let mut cell = Cell::default();
                cell.value = 0.6;
                cell.river_mask = 1.0;
                cell.continent_edge = 0.4;
                map.apply(&mut cell, x, z);
                (
                    cell.value.to_bits(),
                    cell.river_mask.to_bits(),
                    cell.erosion_mask,
                    cell.terrain.id(),
                )
            }),
            [
                (0x3e780000, 0, true, 5),
                (0x3e821ffb, 0x35900000, false, 0),
                (0x3e840402, 0x38e56000, false, 0),
                (0x3e86654e, 0x35f00000, false, 0),
                (0x3e866812, 0x34400000, true, 0),
                (0x3e6401fd, 0x382ac000, true, 5),
                (0x3e87a319, 0x3d40d160, false, 0),
                (0x3f19999a, 0x3f800000, false, 0),
                (0x3f19999a, 0x3f800000, false, 0),
            ]
        );
    }
}
