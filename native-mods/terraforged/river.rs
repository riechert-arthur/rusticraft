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
// lake and river domain warps Engine's river generators build per continent. `River`, `RiverWarp`
// (`rivermap.river`) and `Vec2i` (`noise.util.Vec2i`) are the channel segments those generators lay out, and
// `RiverCarver` (`rivermap.river`) carves one segment's valley, banks and bed into a cell.

use super::{
    terraforged::{
        CurveFunction, Domain, Line, PI2, SourceKind, clamp, cos, java_max, java_min, lerp, map,
        round, simplex, sin,
    },
    terraforged_continent::noise_warp,
    terraforged_engine::{
        Cell, Lake, Levels, Terrain, TerrainProperties, TerrainRegistry, Wetland,
    },
    terraforged_fancy::Vec2f,
    terraforged_util::{pack_floats, unpack_left_float, unpack_right_float},
};
use horizon_core::{
    jmath,
    random::{LegacyRandom, RandomSource},
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

/// Noise's `util.Vec2i`: an integer river node.
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

/// Engine's `rivermap.river.River`: a channel segment with its direction, normal, length and bounds padded
/// by twice the valley radius. As in Engine, a zero-length segment has NaN directions and normals.
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

    /// `new River(x1, z1, x2, z2, radius)`: the length is `(float) Math.sqrt` of the `float` squared length.
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
            min_x: java_min(x1, x2) - radius,
            min_z: java_min(z1, z2) - radius,
            max_x: java_max(x1, x2) + radius,
            max_z: java_max(z1, z2) + radius,
        }
    }

    pub const fn length(self) -> f32 {
        self.length
    }

    /// `getAngle`: `(float) Math.atan2(dx, dz)`.
    pub fn angle(self) -> f32 {
        jmath::atan2(f64::from(self.dx), f64::from(self.dz)) as f32
    }

    /// `pos(distance)`: the point at `distance` (a fraction of the length), packed by `PosUtil.packf`.
    pub fn pos(self, distance: f32) -> i64 {
        pack_floats(self.x1 + self.dx * distance, self.z1 + self.dz * distance)
    }

    /// `pos(distance, warp)`: `pos` minus the warp's offset where `warp.test(distance)`.
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

    /// `intersects(other, extend)`: both segments lengthened at each end by `extend` blocks, at most their length.
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

    /// `shorten`: moves the start `distance` blocks along the river, rounded by `NoiseUtil.round`, with the
    /// default radius.
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

/// Engine's `rivermap.river.RiverWarp`: bends a river sideways by simplex noise, faded in over `0..lower` and out
/// over `upper..1`, plus a wiggle whose amplitude fades in over the first 7.5% of the river. Engine tells
/// `RiverWarp.NONE` apart by identity; here `none()` carries an `enabled: false` flag instead.
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

    /// `createChild`: one `nextInt` draw for the seed, frequency and scale multiplied by `factor`.
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

    /// `getOffset`: the packed (x, z) offset `pos(distance, warp)` subtracts.
    pub fn offset(self, x: f32, z: f32, distance: f32, river: River) -> i64 {
        let noise = simplex(x * self.frequency, z * self.frequency, self.seed);
        let warp_distance = self.warp_alpha(distance) * self.scale;
        let mut dx = river.norm_x * noise * warp_distance;
        let mut dz = river.norm_z * noise * warp_distance;
        let factor = river.length * 0.0004;
        let wiggle_frequency = 8.0 * factor;
        let wiggle_distance = clamp(self.wiggle_alpha(distance) * 25.0 * factor, 2.0, 45.0);
        let radians = noise + distance * PI2 * wiggle_frequency;
        dx += cos(radians) * river.norm_x * wiggle_distance;
        dz += sin(radians) * river.norm_z * wiggle_distance;
        pack_floats(dx, dz)
    }

    pub fn create(fade: f32, random: &mut LegacyRandom) -> Self {
        Self::create_range(fade, 1.0 - fade, random)
    }

    /// `create(lower, upper, random)`: draws `nextInt(50)` (scale), `nextFloat` (frequency), then `nextInt` (seed).
    pub fn create_range(lower: f32, upper: f32, random: &mut LegacyRandom) -> Self {
        let scale = 125.0 + random.next_int_bound(50).expect("positive bound") as f32;
        let frequency = 0.0005 + random.next_float() * 0.0005;
        Self::new(random.next_int(), lower, upper, frequency, scale)
    }

    /// `getWarpAlpha`. Not `!(0.0..=1.0).contains(..)`: a NaN distance falls through to 1, as in Engine.
    #[allow(clippy::manual_range_contains)]
    fn warp_alpha(self, distance: f32) -> f32 {
        if distance < 0.0 || distance > 1.0 {
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

/// Engine's `RiverCarver.Settings`: the valley radius in blocks, the fraction of the river over which
/// widths and depths taper in, and the valley's falloff curve.
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

/// Engine's `RiverCarver`: carves one river segment's valley, banks and bed into a cell's `value`,
/// `river_mask`, `erosion_mask` and `terrain` (nothing else).
///
/// Engine's class extends `TerrainPopulator` (terrain `RIVER`, zero sources, weight 1) only to be
/// typed as one: its `apply` does nothing and nothing reads its populator fields, so they are not
/// ported. Its `compareTo` (by `config.order`) has no caller in Engine and is not ported either.
pub struct RiverCarver {
    pub main: bool,
    /// Engine keeps it but only tests it in an empty `if`.
    _connecting: bool,
    fade: f32,
    fade_inv: f32,
    bed_width: Range,
    banks_width: Range,
    valley_width: Range,
    bed_depth: Range,
    banks_depth: Range,
    water_line: f32,
    /// `TerrainType.RIVER`, from the world's registry so `is_same` checks against its built-ins hold.
    river_terrain: Terrain,
    pub river: River,
    pub warp: RiverWarp,
    pub config: RiverConfig,
    pub valley_curve: CurveFunction,
}

impl RiverCarver {
    /// `new RiverCarver(river, warp, config, settings, levels)`. `registry` is the world's terrain
    /// registry: the carver tags cells with its `river` terrain.
    pub fn new(
        river: River,
        warp: RiverWarp,
        config: RiverConfig,
        settings: RiverCarverSettings,
        levels: Levels,
        registry: &TerrainRegistry,
    ) -> Self {
        let valley_size2 = settings.valley_size * settings.valley_size;
        Self {
            main: config.main,
            _connecting: settings.connecting,
            fade: settings.fade_in,
            fade_inv: 1.0 / settings.fade_in,
            // `int * int`, wrapping, then widened to `float`.
            bed_width: Range::new(0.25, config.bed_width.wrapping_mul(config.bed_width) as f32),
            banks_width: Range::new(
                1.5625,
                config.bank_width.wrapping_mul(config.bank_width) as f32,
            ),
            valley_width: Range::new(valley_size2, valley_size2),
            bed_depth: Range::new(levels.water, config.bed_height),
            banks_depth: Range::new(config.min_bank_height, config.max_bank_height),
            water_line: levels.water,
            river_terrain: registry.get_by_id(5),
            river,
            warp,
            config,
            valley_curve: settings.valley_curve,
        }
    }

    /// `carve(cell, px, pz, pt, x, z, t)`: `(px, pz)` at `pt` is the previous sample along the river,
    /// `(x, z)` at `t` this one.
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
            java_min(distance2, previous_distance2),
            self.valley_width,
        );
        if valley_alpha == 0.0 {
            return;
        }
        let bank_height = self.scaled_size(t, self.banks_depth);
        valley_alpha = self.valley_curve.apply(valley_alpha);
        cell.river_mask = java_min(cell.river_mask, 1.0 - valley_alpha);
        cell.value = java_min(lerp(cell.value, bank_height, valley_alpha), cell.value);

        let mouth_modifier = Self::mouth_modifier(cell);
        let bed_height = self.scaled_size(t, self.bed_depth);
        let banks_alpha = self.distance_alpha(t, distance2 * mouth_modifier, self.banks_width);
        if banks_alpha == 0.0 {
            return;
        }
        if cell.value > bed_height {
            cell.value = java_min(lerp(cell.value, bed_height, banks_alpha), cell.value);
            self.tag(cell, bed_height);
        }
        let bed_alpha = self.distance_alpha(t, distance2, self.bed_width);
        if bed_alpha != 0.0 && cell.value > bed_height {
            cell.value = lerp(cell.value, bed_height, bed_alpha);
            self.tag(cell, bed_height);
        }
    }

    /// `createForkConfig(t, levels)`: a fork sized like this river's bed and banks at `t`.
    pub fn create_fork_config(&self, t: f32, levels: Levels) -> RiverConfig {
        let bed_height = levels.scale_value(self.scaled_size(t, self.bed_depth));
        let bed_width =
            java_round_double(f64::from(self.scaled_size(t, self.bed_width)).sqrt() * 0.75);
        let bank_width =
            java_round_double(f64::from(self.scaled_size(t, self.banks_width)).sqrt() * 0.75);
        let bed_width = bed_width.max(1);
        let bank_width = bank_width.max(bed_width.wrapping_add(1));
        self.config
            .create_fork_sized(bed_height, bed_width, bank_width, levels)
    }

    /// `getValleyType(random)`: one `nextInt(100)`.
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

    /// `RiverCarver.create`: draws the warp (`RiverWarp.create(0.35)`), the valley width
    /// (`MAIN_VALLEY`, one `nextFloat`), then the valley curve, in that order.
    #[allow(clippy::too_many_arguments)]
    pub fn create(
        x1: f32,
        z1: f32,
        x2: f32,
        z2: f32,
        config: RiverConfig,
        levels: Levels,
        registry: &TerrainRegistry,
        random: &mut LegacyRandom,
    ) -> Self {
        let river = River::new(x1, z1, x2, z2);
        let warp = RiverWarp::create(0.35, random);
        let valley_size = 275.0 * River::MAIN_VALLEY.apply(random.next_float());
        let valley_curve = Self::valley_type(random);
        let settings = RiverCarverSettings {
            valley_size,
            fade_in: config.fade,
            connecting: false,
            valley_curve,
        };
        Self::new(river, warp, config, settings, levels, registry)
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

    /// `getScaledSize`, in Engine's order of tests (`min == max` returns `min` before `t >= fade`).
    fn scaled_size(&self, t: f32, range: Range) -> f32 {
        if t < 0.0 {
            range.min
        } else if t > 1.0 {
            range.max
        } else if range.min == range.max {
            range.min
        } else if t >= self.fade {
            range.max
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
        let modifier = map(cell.continent_edge, 0.0, 0.5, 0.5);
        modifier * modifier
    }
}

/// `(int) Math.round(double)`: rounds half up (exactly, unlike `floor(x + 0.5)`), saturates to a
/// `long` (NaN is 0), then keeps the low 32 bits.
fn java_round_double(value: f64) -> i32 {
    if value.is_nan() {
        return 0;
    }
    let floor = value.floor();
    let rounded = if value - floor >= 0.5 {
        floor + 1.0
    } else {
        floor
    };
    (rounded as i64) as i32
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

    /// Folds `River` and `RiverWarp` exactly as `RiverGeometryOracle.java` (`.local/terraforged/oracles/river/`)
    /// folds Engine 0.3.0's classes: zero-length, NaN, infinite and -0 segments, odd radii, distances outside
    /// 0..1 and NaN, warps with NaN and degenerate fades, and `java.util.Random` draws.
    mod geometry_oracle {
        use super::*;

        const NAN: f32 = f32::NAN;
        const INF: f32 = f32::INFINITY;
        const POINTS: [[f32; 2]; 16] = [
            [0.0, 0.0],
            [-0.0, -0.0],
            [10.25, -20.5],
            [310.75, 179.125],
            [100.0, 250.0],
            [200.0, -100.0],
            [-4096.5, 8192.25],
            [1e-3, -1e-3],
            [3.0e7, -3.0e7],
            [16777217.0, 3.0],
            [1e20, -1e30],
            [f32::MAX, -f32::MAX],
            [INF, 0.0],
            [0.0, -INF],
            [NAN, 5.0],
            [5.0, NAN],
        ];
        const RADII: [f32; 9] = [
            275.0,
            300.0,
            0.0,
            -0.0,
            -1.0,
            12.5,
            NAN,
            INF,
            f32::from_bits(1),
        ];
        const T: [f32; 18] = [
            -0.25, -0.0, 0.0, 1e-4, 0.05, 0.075, 0.1, 0.2, 0.25, 0.5, 0.8, 0.9, 1.0, 1.0000001,
            1.25, NAN, INF, -INF,
        ];
        const SHORTEN: [i32; 7] = [0, 1, 100, -50, 2000, i32::MAX, i32::MIN];
        const EXTEND: [f32; 7] = [0.0, -0.0, 50.0, -10.0, 1e9, NAN, INF];
        const WARPS: [[f32; 4]; 8] = [
            [0.2, 0.8, 0.00075, 140.0],
            [0.0, 1.0, 0.0005, 125.0],
            [0.5, 0.5, 0.001, 174.0],
            [0.0, 0.0, 0.0, 0.0],
            [0.9, 0.1, 0.01, -30.0],
            [NAN, 0.75, 0.002, 150.0],
            [0.25, NAN, 0.002, NAN],
            [-0.1, 1.1, 1.0, 1e6],
        ];
        const SEEDS: [i32; 5] = [0, 918_273, -1, i32::MAX, i32::MIN];

        struct Fold(i32);

        impl Fold {
            fn new() -> Self {
                Self(17)
            }
            fn mix(&mut self, v: i32) {
                self.0 = self.0.wrapping_mul(31).wrapping_add(v);
            }
            fn b(&mut self, v: bool) {
                self.mix(i32::from(v));
            }
            /// `Float.floatToIntBits`: every NaN folds as 0x7fc00000.
            fn f(&mut self, v: f32) {
                self.mix(if v.is_nan() {
                    0x7fc0_0000
                } else {
                    v.to_bits() as i32
                });
            }
            fn p(&mut self, packed: i64) {
                self.f(unpack_left_float(packed));
                self.f(unpack_right_float(packed));
            }
            fn river(&mut self, r: River) {
                for v in [
                    r.x1, r.z1, r.x2, r.z2, r.dx, r.dz, r.ndx, r.ndz, r.norm_x, r.norm_z, r.length,
                    r.length2, r.min_x, r.min_z, r.max_x, r.max_z,
                ] {
                    self.f(v);
                }
                self.f(r.length());
                self.f(r.angle());
            }
            fn warp(&mut self, w: RiverWarp) {
                self.mix(w.seed);
                for v in [
                    w.lower,
                    w.upper,
                    w.lower_range,
                    w.upper_range,
                    w.frequency,
                    w.scale,
                ] {
                    self.f(v);
                }
                for t in T {
                    self.b(w.test(t));
                    self.f(w.warp_alpha(t));
                    self.f(w.wiggle_alpha(t));
                }
            }
        }

        fn rivers() -> Vec<River> {
            let mut rivers = Vec::new();
            for a in POINTS {
                for b in POINTS {
                    rivers.push(River::new(a[0], a[1], b[0], b[1]));
                    for radius in RADII {
                        rivers.push(River::with_radius(a[0], a[1], b[0], b[1], radius));
                    }
                }
            }
            rivers
        }

        #[test]
        fn rivers_match_engine_0_3_0() {
            let rivers = rivers();
            assert_eq!(rivers.len(), 2560);
            let mut fold = Fold::new();
            for &r in &rivers {
                fold.river(r);
                for t in T {
                    fold.p(r.pos(t));
                }
                for p in POINTS {
                    fold.b(r.contains(p[0], p[1]));
                }
                for d in SHORTEN {
                    fold.river(r.shorten(d));
                }
            }
            assert_eq!(fold.0, -1_304_354_281);
        }

        #[test]
        fn river_pairs_match_engine_0_3_0() {
            let rivers = rivers();
            let mut fold = Fold::new();
            for &a in rivers.iter().step_by(RADII.len() + 1) {
                for &b in rivers.iter().step_by(RADII.len() + 1) {
                    fold.b(a.intersects(b));
                    for e in EXTEND {
                        fold.b(a.intersects_extended(b, e));
                    }
                    fold.b(a.overlaps(b));
                    fold.b(a.overlaps_bounds(b.min_x, b.min_z, b.max_x, b.max_z));
                }
                for p in POINTS {
                    for radius in RADII {
                        fold.b(a.overlaps_circle(Vec2f::new(p[0], p[1]), radius));
                    }
                }
            }
            assert_eq!(fold.0, -1_335_821_953);
        }

        #[test]
        fn rivers_from_nodes_match_engine_0_3_0() {
            let ints = [0, 1, -1, 300, -300, 16_777_217, i32::MAX, i32::MIN];
            let mut fold = Fold::new();
            for x1 in ints {
                for y1 in ints {
                    for x2 in ints {
                        for y2 in ints {
                            fold.river(River::from_nodes(Vec2i::new(x1, y1), Vec2i::new(x2, y2)));
                        }
                    }
                }
            }
            assert_eq!(fold.0, -933_395_885);
        }

        #[test]
        fn river_warps_match_engine_0_3_0() {
            let rivers = rivers();
            let mut fold = Fold::new();
            for seed in SEEDS {
                for w in WARPS {
                    let warp = RiverWarp::new(seed, w[0], w[1], w[2], w[3]);
                    fold.warp(warp);
                    for &r in rivers.iter().step_by(7) {
                        for t in T {
                            let x = r.x1 + r.dx * t;
                            let z = r.z1 + r.dz * t;
                            fold.p(warp.offset(x, z, t, r));
                            fold.p(r.warped_pos(t, warp));
                        }
                    }
                }
            }
            fold.warp(RiverWarp::none());
            for &r in &rivers {
                for t in T {
                    fold.p(r.warped_pos(t, RiverWarp::none()));
                }
            }
            assert_eq!(fold.0, -2_034_090_640);
        }

        #[test]
        fn river_warp_draws_match_engine_0_3_0() {
            let mut fold = Fold::new();
            for seed in [0, 1, -1, 12345, i64::MAX, i64::MIN] {
                let mut random = LegacyRandom::new(seed);
                let a = RiverWarp::create(0.15, &mut random);
                let b = RiverWarp::create_range(0.1, 0.95, &mut random);
                let c = a.child(0.2, 0.7, 0.5, &mut random);
                let d = RiverWarp::none().child(0.3, 0.6, 2.0, &mut random);
                let e = b.child(NAN, 0.6, NAN, &mut random);
                for w in [a, b, c, d, e] {
                    fold.warp(w);
                }
                fold.mix(random.next_int());
            }
            assert_eq!(fold.0, 2_004_787_916);
        }
    }

    /// Runs `RiverCarver` exactly as `RiverCarverOracle.java` (`.local/terraforged/oracles/river/`) runs
    /// Engine 0.3.0's class: fresh and filled cells (wetland, river, lake, mountain and hill terrains, NaN
    /// values, masks and edges) at points on, beside, before and past four rivers (zero-length and NaN among
    /// them), for degenerate configs and settings (wrapping widths, NaN heights, zero and NaN fades), folding
    /// every cell field after each carve; plus `createForkConfig`, `getValleyType` and `create`'s draws.
    mod carver_oracle {
        use super::*;

        const NAN: f32 = f32::NAN;
        const INF: f32 = f32::INFINITY;
        const RIVERS: [[f32; 4]; 4] = [
            [0.0, 0.0, 1000.0, 0.0],
            [100.0, -50.0, -300.0, 400.0],
            [5.0, 5.0, 5.0, 5.0],
            [0.0, 0.0, NAN, 10.0],
        ];
        #[rustfmt::skip]
        const T: [f32; 16] = [
            -0.25, -0.0, 0.0, 0.05, 0.2, 0.35, 0.5, 0.7, 0.75, 0.9, 1.0, 1.000_000_1, 1.25, NAN, INF, -INF,
        ];
        #[rustfmt::skip]
        const OFFS: [f32; 15] = [
            0.0, -0.0, 0.4, 1.0, 2.5, 4.0, 7.0, 12.0, 19.0, 30.0, 80.0, 150.0, 260.0, 400.0, NAN,
        ];
        /// value, river mask, continent edge, terrain id, erosion mask.
        const FILLS: [(f32, f32, f32, i32, bool); 10] = [
            (0.6, 1.0, 0.4, 0, false),
            (0.3, 0.5, 0.0, 7, false),
            (0.2, NAN, 0.1, 7, true),
            (0.0, -0.0, 0.25, 5, false),
            (1.0, 1.0, 1.0, 12, false),
            (NAN, 1.0, NAN, 6, false),
            (0.24, 0.75, 0.5, 0, false),
            (0.25, 1.0, 0.3, 7, false),
            (-0.5, 1.0, 0.45, 11, false),
            (0.6, 1.0, -1.0, 7, false),
        ];

        struct Fold(i32);

        impl Fold {
            fn mix(&mut self, v: i32) {
                self.0 = self.0.wrapping_mul(31).wrapping_add(v);
            }
            fn b(&mut self, v: bool) {
                self.mix(i32::from(v));
            }
            /// `Float.floatToIntBits`: every NaN folds as 0x7fc00000.
            fn f(&mut self, v: f32) {
                self.mix(if v.is_nan() {
                    0x7fc0_0000
                } else {
                    v.to_bits() as i32
                });
            }
            fn cell(&mut self, c: &Cell, registry: &TerrainRegistry) {
                for v in [
                    c.value,
                    c.erosion,
                    c.sediment,
                    c.gradient,
                    c.moisture,
                    c.temperature,
                    c.continent_id,
                    c.continent_edge,
                    c.terrain_region_id,
                    c.terrain_region_edge,
                    c.biome_region_id,
                    c.biome_region_edge,
                    c.macro_biome_id,
                    c.river_mask,
                ] {
                    self.f(v);
                }
                self.mix(c.terrain_region_center as i32);
                self.mix((c.terrain_region_center as u64 >> 32) as i32);
                self.mix(c.continent_x);
                self.mix(c.continent_z);
                self.b(c.erosion_mask);
                // Engine compares terrains by identity: the cell's must be the world registry's own.
                assert!(c.terrain.is_same(&registry.get_by_id(c.terrain.id())));
                self.mix(c.terrain.id());
                self.mix(c.biome as i32);
            }
            fn config(&mut self, c: &RiverConfig) {
                self.b(c.main);
                self.mix(c.order);
                self.mix(c.bed_width);
                self.mix(c.bank_width);
                self.f(c.bed_height);
                self.f(c.min_bank_height);
                self.f(c.max_bank_height);
                self.mix(c.length);
                self.mix(c.length2);
                self.f(c.fade);
            }
            fn carver(&mut self, k: &RiverCarver) {
                self.b(k.main);
                self.b(k._connecting);
                self.f(k.fade);
                self.f(k.fade_inv);
                for r in [
                    k.bed_width,
                    k.banks_width,
                    k.valley_width,
                    k.bed_depth,
                    k.banks_depth,
                ] {
                    self.f(r.min);
                    self.f(r.max);
                }
                self.f(k.water_line);
                self.config(&k.config);
                for v in [0.0, 0.25, 0.5, 1.0] {
                    self.f(k.valley_curve.apply(v));
                }
                let w = k.warp;
                self.mix(w.seed);
                for v in [
                    w.lower,
                    w.upper,
                    w.lower_range,
                    w.upper_range,
                    w.frequency,
                    w.scale,
                ] {
                    self.f(v);
                }
            }
        }

        fn levels() -> [Levels; 2] {
            [Levels::new(256, 63), Levels::new(63, 255)]
        }

        fn configs(levels: Levels) -> [RiverConfig; 4] {
            let direct =
                |main, order, bed_width, bank_width, bed, min, max, length, length2, fade| {
                    RiverConfig {
                        main,
                        order,
                        bed_width,
                        bank_width,
                        bed_height: bed,
                        min_bank_height: min,
                        max_bank_height: max,
                        length,
                        length2,
                        fade,
                    }
                };
            [
                RiverConfig::builder(levels)
                    .main(true)
                    .bed_width(8)
                    .bank_width(20)
                    .bed_depth(5)
                    .bank_height(2, 6)
                    .fade(0.75)
                    .length(1000)
                    .build(),
                RiverConfig::builder(levels).build(),
                direct(false, 2, 46341, 3, NAN, 0.3, 0.2, 500, 250_000, 0.0),
                direct(true, 7, 1, 1, 0.5, 0.5, 0.5, 100, 10_000, 1.0),
            ]
        }

        fn settings() -> [RiverCarverSettings; 4] {
            let with = |valley_size, fade_in, connecting, valley_curve| RiverCarverSettings {
                valley_size,
                fade_in,
                connecting,
                valley_curve,
            };
            [
                RiverCarverSettings::default(),
                with(100.0, 0.0, true, CurveFunction::s_curve(4.0, 5.0)),
                with(NAN, NAN, false, CurveFunction::s_curve(0.4, 1.0)),
                with(-50.0, 0.35, false, CurveFunction::s_curve(3.0, 0.25)),
            ]
        }

        fn fill(i: Option<usize>, registry: &TerrainRegistry) -> Cell {
            let mut c = Cell::new(registry);
            let Some(i) = i else { return c };
            let (value, river_mask, continent_edge, terrain, erosion_mask) = FILLS[i];
            c.value = value;
            c.river_mask = river_mask;
            c.continent_edge = continent_edge;
            c.terrain = registry.get_by_id(terrain);
            c.erosion_mask = erosion_mask;
            c.erosion = 0.125;
            c.sediment = 0.25;
            c.gradient = 0.5;
            c.moisture = 0.75;
            c.temperature = 0.375;
            c.continent_id = 0.625;
            c.terrain_region_id = 0.875;
            c.terrain_region_edge = 0.0625;
            c.terrain_region_center = 0x0123_4567_89ab_cdef;
            c.biome_region_id = 0.3;
            c.biome_region_edge = 0.7;
            c.macro_biome_id = 0.9;
            c.continent_x = -7;
            c.continent_z = 9;
            c
        }

        #[test]
        fn carves_every_cell_field_like_engine() {
            let registry = TerrainRegistry::new();
            let mut fold = Fold(17);
            let mut carves = 0;
            for levels in levels() {
                for config in configs(levels) {
                    for settings in settings() {
                        for [x1, z1, x2, z2] in RIVERS {
                            let river = River::new(x1, z1, x2, z2);
                            let k = RiverCarver::new(
                                river,
                                RiverWarp::none(),
                                config,
                                settings,
                                levels,
                                &registry,
                            );
                            fold.carver(&k);
                            for t in T {
                                let pt = t - 0.05;
                                for off in OFFS {
                                    let x = river.x1 + t * river.dx + off * river.norm_x;
                                    let z = river.z1 + t * river.dz + off * river.norm_z;
                                    let px = river.x1 + pt * river.dx + off * 0.5 * river.norm_x;
                                    let pz = river.z1 + pt * river.dz + off * 0.5 * river.norm_z;
                                    for i in std::iter::once(None).chain((0..FILLS.len()).map(Some))
                                    {
                                        let mut c = fill(i, &registry);
                                        k.carve(&mut c, px, pz, pt, x, z, t);
                                        fold.cell(&c, &registry);
                                        carves += 1;
                                    }
                                }
                                fold.config(&k.create_fork_config(t, levels));
                            }
                        }
                    }
                }
            }
            assert_eq!((carves, fold.0), (337_920, 1_013_655_601));
        }

        #[test]
        fn samples_match_engine() {
            let registry = TerrainRegistry::new();
            let levels = levels()[0];
            let k = RiverCarver::new(
                River::new(0.0, 0.0, 1000.0, 0.0),
                RiverWarp::none(),
                configs(levels)[0],
                RiverCarverSettings::default(),
                levels,
                &registry,
            );
            let sample = |[px, pz, pt, x, z, t]: [f32; 6]| {
                let mut c = fill(Some(0), &registry);
                k.carve(&mut c, px, pz, pt, x, z, t);
                assert!(c.terrain.is_same(&registry.get_by_id(c.terrain.id())));
                (
                    c.value.to_bits(),
                    c.river_mask.to_bits(),
                    c.erosion_mask,
                    c.terrain.id(),
                )
            };
            assert_eq!(
                [
                    [450.0, 0.0, 0.45, 500.0, 0.0, 0.5],
                    [450.0, 10.0, 0.45, 500.0, 10.0, 0.5],
                    [450.0, 100.0, 0.45, 500.0, 100.0, 0.5],
                    [450.0, 300.0, 0.45, 500.0, 300.0, 0.5],
                    [40.0, 5.0, 0.04, 50.0, 5.0, 0.05],
                    [1050.0, 0.0, 1.05, 1100.0, 0.0, 1.1],
                    [0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
                    [1000.0, 0.0, 1.0, 1000.0, 0.0, 1.0],
                ]
                .map(sample),
                [
                    (0x3e69b6db, 0, true, 5),
                    (0x3e71688f, 0x3b020100, true, 5),
                    (0x3ea84507, 0x3e4bf690, false, 0),
                    (0x3f19999a, 0x3f800000, false, 0),
                    (0x3e7c4cbc, 0x3a01fc00, true, 0),
                    (0x3e908257, 0x3d4b9540, false, 0),
                    (0x3e780000, 0, true, 5),
                    (0x3e640000, 0, true, 5),
                ]
            );
            // A NaN distance or `t` makes every alpha NaN: `Math.min` keeps the NaN in the mask and value.
            for p in [
                [NAN, 0.0, 0.5, 500.0, 0.0, 0.5],
                [500.0, 0.0, NAN, 500.0, 0.0, NAN],
            ] {
                let (value, mask, erosion_mask, terrain) = sample(p);
                assert!(f32::from_bits(value).is_nan() && f32::from_bits(mask).is_nan());
                assert_eq!((erosion_mask, terrain), (false, 0));
            }
        }

        #[test]
        fn draws_like_engine() {
            let registry = TerrainRegistry::new();
            let levels = levels()[0];
            let mut fold = Fold(17);
            let seeds = [0, 1, 2, 3, 4, 12_345, -1, i64::MAX, i64::MIN];
            for seed in seeds {
                let mut random = LegacyRandom::new(seed);
                let curve = RiverCarver::valley_type(&mut random);
                fold.f(curve.apply(0.5));
                fold.mix(random.next_int());
            }
            for seed in 0..64 {
                fold.f(RiverCarver::valley_type(&mut LegacyRandom::new(seed)).apply(0.3));
            }
            for seed in seeds {
                let mut random = LegacyRandom::new(seed);
                for config in configs(levels) {
                    let k = RiverCarver::create(
                        10.0,
                        -20.0,
                        900.0,
                        300.0,
                        config,
                        levels,
                        &registry,
                        &mut random,
                    );
                    fold.carver(&k);
                }
                fold.mix(random.next_int());
            }
            assert_eq!(fold.0, 1_277_683_933);
        }
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
