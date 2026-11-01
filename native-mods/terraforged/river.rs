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
    use super::super::terraforged_engine::RiverSettings as DefaultRiverSettings;
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
}
