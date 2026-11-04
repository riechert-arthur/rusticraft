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
// `RiverCarver` (`rivermap.river`) carves one segment's valley, banks and bed into a cell; `LakeCarver`
// (`rivermap.lake.Lake`) carves a lake and its valley, `WetlandCarver` (`rivermap.wetland.Wetland`) a wetland
// along a segment, and `Boundsf` (`util.Boundsf`) bounds them. `RiverNetwork` (`rivermap.river.Network`) groups a
// river with its lakes, wetlands and forks, `Rivermap` (`rivermap.Rivermap`) holds one continent's networks, and
// `BaseRiverGenerator` (`rivermap.river.BaseRiverGenerator`) lays out the forks, lakes and wetlands of a map.

use super::{
    terraforged::{
        CurveFunction, Domain, Line, Module, PI2, SourceKind, clamp, cos, java_max, java_min, lerp,
        map, round, simplex, sin,
    },
    terraforged_continent::noise_warp,
    terraforged_engine::{
        Cell, Lake, Levels, RiverSettings, Terrain, TerrainProperties, TerrainRegistry, Wetland,
        WorldSettings,
    },
    terraforged_fancy::Vec2f,
    terraforged_landforms::source,
    terraforged_util::{pack_floats, unpack_left_float, unpack_right_float},
};
use horizon_core::{
    jmath,
    random::{LegacyRandom, RandomSource},
};
use std::{
    collections::HashMap,
    sync::{
        Arc, PoisonError, RwLock,
        atomic::{AtomicI64, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
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

    pub fn next(self, random: &mut LegacyRandom) -> f32 {
        self.apply(random.next_float())
    }

    pub fn next_scaled(self, random: &mut LegacyRandom, scaler: f32) -> f32 {
        self.apply_scaled(random.next_float(), scaler)
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

/// Engine's `util.Boundsf`: a float rectangle, inclusive at both ends (a NaN test is never contained).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Boundsf {
    pub min_x: f32,
    pub min_y: f32,
    pub max_x: f32,
    pub max_y: f32,
}

impl Boundsf {
    /// `Boundsf.NONE`: min above max, so it contains nothing.
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

/// Engine's `Boundsf.Builder`. As in Engine, the maxima start at `Float.MIN_VALUE` (the smallest positive
/// subnormal, not `f32::MIN_POSITIVE`), so an empty builder or one that only records negative coordinates
/// keeps that maximum; `record` uses `Math.min`/`Math.max`, so a NaN coordinate sticks.
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
            max_x: f32::from_bits(1),
            max_y: f32::from_bits(1),
        }
    }

    pub fn record(&mut self, x: f32, y: f32) {
        self.min_x = java_min(self.min_x, x);
        self.min_y = java_min(self.min_y, y);
        self.max_x = java_max(self.max_x, x);
        self.max_y = java_max(self.max_y, y);
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

/// Engine's `rivermap.lake.Lake` (named `LakeCarver` here: `Lake` is the settings struct): a circular lake
/// bed of radius `radius * multiplier` inside a valley of radius `275 * multiplier`. `apply` writes only the
/// cell's `value`, `river_mask` and `terrain`, in Engine's order.
///
/// Engine's class extends `TerrainPopulator` (terrain `LAKE`, zero sources, weight 1) only to be typed as one;
/// nothing reads those populator fields, so they are not ported.
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
    /// `TerrainType.LAKE`, from the world's registry so `is_same` checks against its built-ins hold.
    lake_terrain: Terrain,
}

impl LakeCarver {
    /// `new Lake(center, radius, multiplier, config)`. `registry` is the world's terrain registry: the lake
    /// tags cells with its `lake` terrain.
    pub fn new(
        center: Vec2f,
        radius: f32,
        multiplier: f32,
        config: LakeConfig,
        registry: &TerrainRegistry,
    ) -> Self {
        let lake = radius * multiplier;
        let valley = 275.0 * multiplier;
        let valley2 = valley * valley;
        let bank_alpha_min = config.bank_min;
        let bank_alpha_max = java_min(1.0, bank_alpha_min + 0.275);
        let lake_distance2 = lake * lake;
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
            lake_terrain: registry.get_by_id(6),
        }
    }

    /// `apply(cell, x, z)`. Engine clamps its alphas with `<`/`>` tests, so a NaN alpha passes through.
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
            cell.river_mask = java_min(cell.river_mask, 1.0 - valley_alpha);
            return;
        }
        cell.value = java_min(bank_height, cell.value);
        if distance2 < self.lake_distance2 {
            let depth_alpha = clamp(1.0 - distance2 / self.lake_distance2, 0.0, 1.0);
            let lake_depth = java_min(cell.value, self.depth);
            cell.value = lerp(cell.value, lake_depth, depth_alpha);
            cell.terrain = self.lake_terrain.clone();
            cell.river_mask = java_min(cell.river_mask, 1.0 - depth_alpha);
        }
    }

    /// `recordBounds(builder)`: the valley's square, widened by 20%.
    pub fn record_bounds(&self, builder: &mut BoundsfBuilder) {
        let radius = self.valley * 1.2;
        builder.record(self.center.x - radius, self.center.y - radius);
        builder.record(self.center.x + radius, self.center.y + radius);
    }

    /// `overlaps(x, z, radius2)`: whether `(x, z)` is closer than the lake's squared radius plus `radius2`.
    pub fn overlaps(&self, x: f32, z: f32, radius2: f32) -> bool {
        self.distance2(x, z) < self.lake_distance2 + radius2
    }

    fn distance2(&self, x: f32, z: f32) -> f32 {
        let dx = self.center.x - x;
        let dz = self.center.y - z;
        dx * dx + dz * dz
    }

    fn bank_height(&self, cell: &Cell) -> f32 {
        let alpha = map(
            cell.value,
            self.bank_alpha_min,
            self.bank_alpha_max,
            self.bank_alpha_range,
        );
        lerp(self.bank_min, self.bank_max, alpha)
    }
}

/// Engine's `rivermap.wetland.Wetland` (named `WetlandCarver` here: `Wetland` is the settings struct): a valley of
/// `radius` around the segment `start..end`, with pools, banks and seeded mound noise. `apply_warped` measures the
/// distance to the segment at the (warped) river coordinates and samples the noise at the terrain coordinates;
/// `apply` uses the same coordinates for both. It writes only the cell's `value`, `erosion_mask`, `terrain` and
/// `river_mask`, in Engine's order.
///
/// Engine's class extends `TerrainPopulator` (terrain `WETLAND`, zero sources, weight 1) only to be typed as one
/// and to hand `getType()` to the cells it tags; the other populator fields are unused, so they are not ported.
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
    /// `TerrainType.WETLAND`, from the world's registry so `is_same` checks against its built-ins hold.
    wetland_terrain: Terrain,
}

impl WetlandCarver {
    /// `new Wetland(seed, a, b, radius, levels)`. `registry` is the world's terrain registry: the wetland tags
    /// cells with its `wetland` terrain. The three noises take `++seed` in turn (wrapping like Java's `int`).
    pub fn new(
        seed: i32,
        start: Vec2f,
        end: Vec2f,
        radius: f32,
        levels: Levels,
        registry: &TerrainRegistry,
    ) -> Self {
        let mound_min = levels.water(1);
        let mound_max = levels.water(2);
        // `Source.perlin/simplex(seed, scale, 1)`: Builder defaults (gain 0.5, lacunarity 2, Curve3).
        let noise = |kind, seed, scale, min, max, range| {
            source(kind, seed, scale, 1, 0.5, 2.0)
                .clamp(Module::constant(min), Module::constant(max))
                .map(Module::constant(0.0), Module::constant(range))
        };
        Self {
            start,
            end,
            radius,
            radius2: radius * radius,
            bed: levels.water(-1) - 0.5 / levels.world_height as f32,
            banks: levels.ground(3),
            mound_min,
            mound_max,
            mound_variance: mound_max - mound_min,
            mound_shape: noise(SourceKind::Perlin, seed.wrapping_add(1), 10, 0.3, 0.6, 1.0),
            mound_height: noise(SourceKind::Simplex, seed.wrapping_add(2), 20, 0.0, 0.3, 1.0),
            terrain_edge: noise(SourceKind::Perlin, seed.wrapping_add(3), 8, 0.2, 0.8, 0.9),
            wetland_terrain: registry.get_by_id(7),
        }
    }

    /// `apply(cell, x, z)`.
    pub fn apply(&self, cell: &mut Cell, x: f32, z: f32) {
        self.apply_warped(cell, x, z, x, z);
    }

    /// `apply(cell, rx, rz, x, z)`. Engine's tests are `<`/`>`/`<=`/`>=`, so a NaN value or distance passes them
    /// as Java's do.
    pub fn apply_warped(&self, cell: &mut Cell, river_x: f32, river_z: f32, x: f32, z: f32) {
        if cell.value < self.bed {
            return;
        }
        let (a, b) = (self.start, self.end);
        let t = Line::distance_on_line(river_x, river_z, a.x, a.y, b.x, b.y);
        let distance2 = if t <= 0.0 {
            Line::dist2(river_x, river_z, a.x, a.y)
        } else if t >= 1.0 {
            Line::dist2(river_x, river_z, b.x, b.y)
        } else {
            Line::dist2(
                river_x,
                river_z,
                a.x + t * (b.x - a.x),
                a.y + t * (b.y - a.y),
            )
        };
        if distance2 > self.radius2 {
            return;
        }
        let distance = 1.0 - distance2 / self.radius2;
        if distance <= 0.0 {
            return;
        }
        let valley_alpha = map(distance, 0.0, 0.65, 0.65);
        if cell.value > self.banks {
            cell.value = lerp(cell.value, self.banks, valley_alpha);
        }
        let pools_alpha = map(distance, 0.65, 0.7, 0.050_000_012);
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
        cell.river_mask = java_min(cell.river_mask, 1.0 - valley_alpha);
    }

    /// `recordBounds(builder)`: the segment's box widened by `radius`.
    pub fn record_bounds(&self, builder: &mut BoundsfBuilder) {
        let (a, b) = (self.start, self.end);
        builder.record(
            java_min(a.x, b.x) - self.radius,
            java_min(a.y, b.y) - self.radius,
        );
        builder.record(
            java_max(a.x, b.x) + self.radius,
            java_max(a.y, b.y) + self.radius,
        );
    }
}

/// Engine's `rivermap.river.Network`: a river carver with its lakes, wetlands and child networks. Only the root
/// has real bounds; `RiverNetworkBuilder::build` gives every child `Boundsf::NONE`, as Engine does.
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

/// Engine's `Network.Builder`. Its `min/max` box (`add_bounds`, Engine's `Math.min`/`Math.max`) is kept as in
/// Engine, which never reads it: `build` takes the bounds from the rivers, lakes and wetlands.
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
        self._min_x = java_min(java_min(self._min_x, river.x1), river.x2);
        self._min_z = java_min(java_min(self._min_z, river.z1), river.z2);
        self._max_x = java_max(java_max(self._max_x, river.x1), river.x2);
        self._max_z = java_max(java_max(self._max_z, river.z1), river.z2);
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

/// Engine's `rivermap.Rivermap`: one continent's river networks (at continent cell `x`, `z`) with the
/// `GenWarp` they are carved through, and its creation time for the rivermap cache (`ExpiringEntry`).
/// Engine's static `Rivermap.get` (reuse an instance or ask the heightmap's continent) comes with the
/// heightmap wiring.
pub struct Rivermap {
    x: i32,
    z: i32,
    lake_warp: Domain,
    river_warp: Domain,
    networks: Vec<RiverNetwork>,
    timestamp: i64,
}

/// `System.currentTimeMillis()`.
fn current_time_millis() -> i64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(since) => i64::try_from(since.as_millis()).unwrap_or(i64::MAX),
        Err(before) => i64::try_from(before.duration().as_millis()).map_or(i64::MIN, |ms| -ms),
    }
}

impl Rivermap {
    /// `new Rivermap(x, z, networks, warp)`; the timestamp is `System.currentTimeMillis()`.
    pub fn new(x: i32, z: i32, networks: Vec<RiverNetwork>, warp: GenWarp) -> Self {
        Self {
            x,
            z,
            lake_warp: warp.lake,
            river_warp: warp.river,
            networks,
            timestamp: current_time_millis(),
        }
    }

    /// `apply(cell, x, z)`: warps `(x, z)` with the river warp, takes the lake offset at the warped point
    /// and carves every network whose bounds contain the warped point, in order.
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

    /// `getTimestamp()`: milliseconds since the epoch when the map was made.
    pub const fn timestamp(&self) -> i64 {
        self.timestamp
    }

    pub const fn x(&self) -> i32 {
        self.x
    }

    pub const fn z(&self) -> i32 {
        self.z
    }
}

/// Engine's `rivermap.RiverCache`: river maps by continent cell, made by a `RiverGenerator` on first use and kept
/// until they are more than five minutes old (`Cache("RiverCache", 32, 5, 1, MINUTES, StampedLongMap::new)`).
///
/// Shared across threads like Engine's `StampedLongMap`: lookups that hit take a read lock, a miss takes the write
/// lock and generates under it, so each map is generated once and other lookups wait meanwhile. Engine's
/// `CacheManager` sweeps expired maps from a scheduler thread every minute; here the sweep runs on a miss, at most
/// once a minute, so the cache holds no more than the maps made in the last six minutes or so (hits add nothing).
/// A map is the same whenever it is regenerated, so when the sweep runs never changes what the cache returns.
pub struct RiverCache {
    entries: RwLock<HashMap<i64, Arc<Rivermap>>>,
    /// `System.currentTimeMillis()` of the last sweep (0: none yet).
    last_sweep: AtomicI64,
}

impl RiverCache {
    /// The cache's lifetime: 5 minutes.
    const LIFETIME_MILLIS: i64 = 5 * 60 * 1_000;
    /// How often Engine's scheduler sweeps it: every minute.
    const SWEEP_INTERVAL_MILLIS: u64 = 60 * 1_000;

    pub fn new() -> Self {
        Self {
            entries: RwLock::new(HashMap::new()),
            last_sweep: AtomicI64::new(0),
        }
    }

    /// `getRivers(x, z)`: the map of continent cell (`x`, `z`), generated as `generate(x, z, PosUtil.pack(x, z))`
    /// on a miss.
    pub fn get_rivers(
        &self,
        x: i32,
        z: i32,
        generate: impl FnOnce(i32, i32, i64) -> Rivermap,
    ) -> Arc<Rivermap> {
        self.get_or_generate(super::terraforged_util::pack(x, z), x, z, generate)
    }

    /// `StampedLongMap.computeIfAbsent`: a read-locked lookup, then a write-locked `computeIfAbsent`.
    fn get_or_generate(
        &self,
        id: i64,
        x: i32,
        z: i32,
        generate: impl FnOnce(i32, i32, i64) -> Rivermap,
    ) -> Arc<Rivermap> {
        // A generator that panicked under the lock inserted nothing, so a poisoned map is still consistent.
        if let Some(map) = self
            .entries
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&id)
        {
            return Arc::clone(map);
        }
        let mut entries = self.entries.write().unwrap_or_else(PoisonError::into_inner);
        let now = current_time_millis();
        // `abs_diff`: a clock set back must not stop the sweeps until it catches up.
        if now.abs_diff(self.last_sweep.load(Ordering::Relaxed)) >= Self::SWEEP_INTERVAL_MILLIS {
            self.last_sweep.store(now, Ordering::Relaxed);
            // `Cache.run` and `Cache.test`: a map expires once its timestamp is before `now - lifetime`.
            let timeout = now.saturating_sub(Self::LIFETIME_MILLIS);
            entries.retain(|_, map| map.timestamp() >= timeout);
        }
        Arc::clone(
            entries
                .entry(id)
                .or_insert_with(|| Arc::new(generate(x, z, id))),
        )
    }
}

impl Default for RiverCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Engine's `rivermap.LegacyRiverCache` (the simple continent's): a [`RiverCache`] keyed by `NoiseUtil.seed(x, z)`
/// (`z` in the high half, unlike `PosUtil.pack`), which also passes its generator that id.
#[derive(Default)]
pub struct LegacyRiverCache {
    cache: RiverCache,
}

impl LegacyRiverCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// `getRivers(x, z)`: the map of continent cell (`x`, `z`), generated as `generate(x, z, NoiseUtil.seed(x, z))`
    /// on a miss.
    pub fn get_rivers(
        &self,
        x: i32,
        z: i32,
        generate: impl FnOnce(i32, i32, i64) -> Rivermap,
    ) -> Arc<Rivermap> {
        let id = (i64::from(z) << 32) | (i64::from(x) & 0xffff_ffff);
        self.cache.get_or_generate(id, x, z, generate)
    }
}

/// The part of Engine's `Continent` the river generators read: `getEdgeValue(x, z)`.
pub trait ContinentSampler {
    fn edge_value(&self, x: f32, z: f32) -> f32;
}

impl<T: ContinentSampler + ?Sized> ContinentSampler for &T {
    fn edge_value(&self, x: f32, z: f32) -> f32 {
        T::edge_value(self, x, z)
    }
}

impl ContinentSampler for super::terraforged_fancy::FancyContinentGenerator {
    fn edge_value(&self, x: f32, z: f32) -> f32 {
        super::terraforged_fancy::FancyContinentGenerator::edge_value(self, x, z)
    }
}

/// Engine's `rivermap.river.BaseRiverGenerator`: the river, fork, lake and wetland configs of a world and the
/// fork, lake and wetland placement its subclasses share. Engine reads everything from a `GeneratorContext`:
/// here the caller passes `context.seed.root()`, `context.settings.world`, `context.settings.rivers`,
/// `context.levels` and the world's terrain registry (whose `river`, `lake` and `wetland` terrains the carvers
/// tag cells with).
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
    pub registry: Arc<TerrainRegistry>,
}

impl<C: ContinentSampler> BaseRiverGenerator<C> {
    pub fn new(
        continent: C,
        root_seed: i32,
        world: &WorldSettings,
        rivers: &RiverSettings,
        levels: Levels,
        registry: Arc<TerrainRegistry>,
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
            registry,
        }
    }

    /// `generateRivers(x, z, id)`. `roots` is the subclass's `generateRoots(x, z, random, warp)` (Engine's base
    /// class returns no roots). The roots are shuffled with `Collections.shuffle`, forked (each fork tested for
    /// overlaps against every root's tree as built so far), then given wetlands.
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
        for index in 0..rivers.len() {
            self.generate_forks(
                &mut rivers,
                &[index],
                River::MAIN_SPACING,
                self.fork,
                &mut random,
                &warp,
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

    /// `generateForks(parent, spacing, config, random, warp, rivers, depth)`. Engine's `parent` is a builder
    /// inside `rivers`' trees, which it grows while testing new forks against them; here `parent` is that
    /// builder's path in `rivers`: its root's index, then a child index per level. Each fork is added to its
    /// parent before its own forks are generated, as in Engine.
    // `!(edge < min)` keeps Engine's NaN behavior, which `edge >= min` would not.
    #[allow(clippy::too_many_arguments, clippy::neg_cmp_op_on_partial_ord)]
    pub fn generate_forks(
        &self,
        rivers: &mut [RiverNetworkBuilder],
        parent: &[usize],
        spacing: Variance,
        config: RiverConfig,
        random: &mut LegacyRandom,
        warp: &GenWarp,
        depth: i32,
    ) {
        if depth > 2 {
            return;
        }
        let parent_river = builder_at(rivers, parent).carver.river;
        let length = 0.44 * parent_river.length;
        if length < 300.0 {
            return;
        }
        let mut direction: i32 = if random.next_boolean() { 1 } else { -1 };
        let mut offset = 0.25_f32;
        while offset < 0.9 {
            direction = -direction;
            let parent_angle = parent_river.angle();
            let fork_angle = direction as f32 * PI2 * River::FORK_ANGLE.next(random);
            let angle = parent_angle + fork_angle;
            let dx = sin(angle);
            let dz = cos(angle);
            let start = parent_river.pos(offset);
            let x1 = unpack_left_float(start);
            let z1 = unpack_right_float(start);
            // `!(edge < min)`, so a NaN edge value passes, as in Engine.
            if !(self.continent.edge_value(x1, z1) < self.min_edge_value) {
                let x2 = x1 - dx * length;
                let z2 = z1 - dz * length;
                if !(self.continent.edge_value(x2, z2) < self.min_edge_value) {
                    let builder = builder_at(rivers, parent);
                    let fork_config = builder.carver.create_fork_config(offset, self.levels);
                    let river = River::new(x2, z2, x1, z1);
                    if !Self::river_overlaps(river, Some(builder), rivers) {
                        let valley_size =
                            River::VALLEY_WIDTH as f32 * River::FORK_VALLEY.next(random);
                        let settings = RiverCarverSettings {
                            connecting: true,
                            fade_in: config.fade,
                            valley_size,
                            valley_curve: RiverCarver::valley_type(random),
                        };
                        let fork_warp = builder.carver.warp.child(0.15, 0.75, 0.65, random);
                        let fork = RiverCarver::new(
                            river,
                            fork_warp,
                            fork_config,
                            settings,
                            self.levels,
                            &self.registry,
                        );
                        let children = &mut builder_at_mut(rivers, parent).children;
                        children.push(RiverNetwork::builder(fork));
                        let mut path = parent.to_vec();
                        path.push(children.len() - 1);
                        self.generate_forks(
                            rivers,
                            &path,
                            River::FORK_SPACING,
                            config,
                            random,
                            warp,
                            depth + 1,
                        );
                    }
                }
            }
            offset += spacing.next(random);
        }
        self.add_lake(builder_at_mut(rivers, parent), random, warp);
    }

    /// `riverOverlaps(river, parent, rivers)`: whether `river` overlaps any builder in `rivers`' trees, with
    /// `parent` (compared by identity; `None` is Engine's `null`) tested by `Network.Builder.overlaps`'s
    /// parent rule.
    pub fn river_overlaps(
        river: River,
        parent: Option<&RiverNetworkBuilder>,
        rivers: &[RiverNetworkBuilder],
    ) -> bool {
        rivers
            .iter()
            .any(|other| other.overlaps(river, parent, 250.0))
    }

    /// `generateAdditionalLakes(x, z, random, roots, rivers, lakes)`. Engine never calls it; as in Engine, every
    /// lake is placed at angle 0 and `angleVariance` is unused.
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
        let distance_variance = Variance::of(0.6, 0.3);
        for index in 1..roots.len() {
            let first = &roots[index - 1];
            let angle = 0.0;
            let dx = sin(angle);
            let dz = cos(angle);
            let distance = distance_variance.next(random);
            let lake_x = x as f32 + dx * first.carver.river.length * distance;
            let lake_z = z as f32 + dz * first.carver.river.length * distance;
            let variance = size_variance.next(random);
            let center = Vec2f::new(lake_x, lake_z);
            if Self::lake_overlaps(center, size, rivers) {
                continue;
            }
            lakes.push(LakeCarver::new(
                center,
                size,
                variance,
                self.lake,
                &self.registry,
            ));
        }
    }

    /// `generateWetlands(builder, random)`: one `nextInt(skipSize)`, then, on 0, a wetland along part of the
    /// river; then the children, depth first.
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
            let start = Vec2f::new(unpack_left_float(start), unpack_right_float(start));
            let end = Vec2f::new(unpack_left_float(end), unpack_right_float(end));
            builder.wetlands.push(WetlandCarver::new(
                random.next_int(),
                start,
                end,
                width,
                self.levels,
                &self.registry,
            ));
        }
        for child in &mut builder.children {
            self.generate_wetlands(child, random);
        }
    }

    /// `addLake(branch, random, warp)`: with the lake chance, a lake at the branch's start unless it overlaps
    /// one of the branch's lakes. `warp` is unused, as in Engine.
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
            if Self::lake_overlaps_other(center_x, center_z, lake_size, &branch.lakes) {
                return;
            }
            branch.lakes.push(LakeCarver::new(
                Vec2f::new(center_x, center_z),
                lake_size,
                1.0,
                self.lake,
                &self.registry,
            ));
        }
    }

    /// `lakeOverlaps(lake, size, rivers)`: whether a non-main river's box overlaps the lake's square.
    pub fn lake_overlaps(lake: Vec2f, size: f32, rivers: &[RiverCarver]) -> bool {
        rivers
            .iter()
            .any(|other| !other.main && other.river.overlaps_circle(lake, size))
    }

    /// `lakeOverlapsOther(x, z, size, lakes)`.
    pub fn lake_overlaps_other(x: f32, z: f32, size: f32, lakes: &[LakeCarver]) -> bool {
        let distance2 = size * size;
        lakes.iter().any(|other| other.overlaps(x, z, distance2))
    }

    /// `BaseRiverGenerator.create(x1, z1, x2, z2, config, levels, random)` with this generator's levels and
    /// registry (Engine's static method draws exactly as `RiverCarver::create`).
    pub fn create_main(
        &self,
        x1: f32,
        z1: f32,
        x2: f32,
        z2: f32,
        config: RiverConfig,
        random: &mut LegacyRandom,
    ) -> RiverCarver {
        RiverCarver::create(x1, z1, x2, z2, config, self.levels, &self.registry, random)
    }

    /// `BaseRiverGenerator.createFork(x1, z1, x2, z2, valleyWidth, config, levels, random)` with this
    /// generator's levels and registry: draws the warp (`RiverWarp.create(0.4)`), then the valley curve.
    #[allow(clippy::too_many_arguments)]
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
        let river = River::new(x1, z1, x2, z2);
        let warp = RiverWarp::create(0.4, random);
        let settings = RiverCarverSettings {
            connecting: true,
            fade_in: config.fade,
            valley_size,
            valley_curve: RiverCarver::valley_type(random),
        };
        RiverCarver::new(river, warp, config, settings, self.levels, &self.registry)
    }
}

fn builder_at<'a>(rivers: &'a [RiverNetworkBuilder], path: &[usize]) -> &'a RiverNetworkBuilder {
    let (&root, rest) = path.split_first().expect("non-empty builder path");
    rest.iter()
        .fold(&rivers[root], |builder, &child| &builder.children[child])
}

fn builder_at_mut<'a>(
    rivers: &'a mut [RiverNetworkBuilder],
    path: &[usize],
) -> &'a mut RiverNetworkBuilder {
    let (&root, rest) = path.split_first().expect("non-empty builder path");
    rest.iter().fold(&mut rivers[root], |builder, &child| {
        &mut builder.children[child]
    })
}

/// `Collections.shuffle(list, random)`: from the last index down to 1, swap with `nextInt(i + 1)`.
fn java_shuffle<T>(values: &mut [T], random: &mut LegacyRandom) {
    for index in (1..values.len()).rev() {
        let swap = random
            .next_int_bound((index + 1) as i32)
            .expect("positive shuffle bound") as usize;
        values.swap(index, swap);
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
            pub(super) fn b(&mut self, v: bool) {
                self.mix(i32::from(v));
            }
            /// `Float.floatToIntBits`: every NaN folds as 0x7fc00000.
            pub(super) fn f(&mut self, v: f32) {
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

        pub(super) struct Fold(pub(super) i32);

        impl Fold {
            pub(super) fn mix(&mut self, v: i32) {
                self.0 = self.0.wrapping_mul(31).wrapping_add(v);
            }
            pub(super) fn b(&mut self, v: bool) {
                self.mix(i32::from(v));
            }
            /// `Float.floatToIntBits`: every NaN folds as 0x7fc00000.
            pub(super) fn f(&mut self, v: f32) {
                self.mix(if v.is_nan() {
                    0x7fc0_0000
                } else {
                    v.to_bits() as i32
                });
            }
            pub(super) fn cell(&mut self, c: &Cell, registry: &TerrainRegistry) {
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

    /// Engine 0.3.0's `Lake` and `Boundsf` against `LakeOracle.java` (`.local/terraforged/oracles/river/`,
    /// `run-lake.sh`), which runs Engine's classes on the same inputs.
    mod lake_oracle {
        use super::*;

        const NAN: f32 = f32::NAN;
        const INF: f32 = f32::INFINITY;
        /// depth, min bank height, max bank height.
        const SETTINGS: [(i32, i32, i32); 6] = [
            (10, 2, 10),
            (1, 1, 1),
            (20, 10, 1),
            (0, 0, 0),
            (-5, 300, -300),
            (i32::MIN, i32::MAX, i32::MIN),
        ];
        const CENTERS: [[f32; 2]; 3] = [[100.0, -50.0], [-0.0, -0.0], [1e7, -3.5e6]];
        /// radius, multiplier.
        #[rustfmt::skip]
        const SIZES: [[f32; 2]; 10] = [
            [75.0, 0.8], [150.0, 1.0], [0.0, 1.0], [75.0, 0.0], [100.0, -1.0], [NAN, 1.0], [75.0, NAN],
            [INF, 1.0], [300.0, 1.0], [75.0, INF],
        ];
        #[rustfmt::skip]
        const OFFS: [f32; 20] = [
            0.0, -0.0, 1.0, 30.0, 59.999_996, 60.0, 60.000_004, 100.0, 150.0, 219.999_98, 220.0, 220.000_02,
            275.0, 300.0, 330.0, 500.0, 1e20, NAN, INF, -INF,
        ];
        const RADII2: [f32; 7] = [0.0, -0.0, 400.0, -1.0, 1e9, NAN, INF];
        /// value, river mask, terrain id (erosion mask: even fills).
        #[rustfmt::skip]
        const FILLS: [(f32, f32, i32); 12] = [
            (0.6, 1.0, 0), (0.3, 0.5, 7), (0.2, NAN, 5), (0.0, -0.0, 6), (1.0, 1.0, 12), (NAN, 1.0, 0),
            (0.24, 0.75, 8), (0.25, 1.0, 7), (-0.5, 1.0, 11), (-0.0, 2.0, 0), (INF, -1.0, 0),
            (0.242_187_5, 0.25, 3),
        ];

        pub(super) struct Fold(pub(super) i32);

        impl Fold {
            pub(super) fn mix(&mut self, v: i32) {
                self.0 = self.0.wrapping_mul(31).wrapping_add(v);
            }
            pub(super) fn b(&mut self, v: bool) {
                self.mix(i32::from(v));
            }
            /// `Float.floatToIntBits`: every NaN folds as 0x7fc00000.
            pub(super) fn f(&mut self, v: f32) {
                self.mix(if v.is_nan() {
                    0x7fc0_0000
                } else {
                    v.to_bits() as i32
                });
            }
            pub(super) fn cell(&mut self, c: &Cell, registry: &TerrainRegistry) {
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
            fn lake(&mut self, k: &LakeCarver) {
                for v in [
                    k.valley,
                    k.valley2,
                    k.lake_distance2,
                    k.valley_distance2,
                    k.bank_alpha_min,
                    k.bank_alpha_max,
                    k.bank_alpha_range,
                    k.depth,
                    k.bank_min,
                    k.bank_max,
                    k.center.x,
                    k.center.y,
                ] {
                    self.f(v);
                }
            }
            pub(super) fn bounds(&mut self, b: Boundsf) {
                for v in [b.min_x, b.min_y, b.max_x, b.max_y] {
                    self.f(v);
                }
            }
        }

        pub(super) fn levels() -> [Levels; 3] {
            // TerraForged's own, swapped `Levels(63, 255)` among them.
            [
                Levels::new(256, 63),
                Levels::new(63, 255),
                Levels::new(384, 62),
            ]
        }

        fn configs(levels: Levels) -> Vec<LakeConfig> {
            let mut configs: Vec<_> = SETTINGS
                .iter()
                .map(|&(depth, min_bank_height, max_bank_height)| {
                    LakeConfig::new(
                        Lake {
                            depth,
                            min_bank_height,
                            max_bank_height,
                            ..Lake::default()
                        },
                        levels,
                    )
                })
                .collect();
            let default = LakeConfig::new(Lake::default(), levels);
            for (depth, bank_min, bank_max) in [(NAN, NAN, INF), (-0.0, 0.9, -INF)] {
                configs.push(LakeConfig {
                    depth,
                    bank_min,
                    bank_max,
                    ..default
                });
            }
            configs
        }

        fn fill(i: Option<usize>, registry: &TerrainRegistry) -> Cell {
            fill_from(&FILLS, i, registry)
        }

        /// A fresh cell (`None`) or `fills[i]` with every other field set.
        pub(super) fn fill_from(
            fills: &[(f32, f32, i32)],
            i: Option<usize>,
            registry: &TerrainRegistry,
        ) -> Cell {
            let mut c = Cell::new(registry);
            let Some(i) = i else { return c };
            let (value, river_mask, terrain) = fills[i];
            c.value = value;
            c.river_mask = river_mask;
            c.terrain = registry.get_by_id(terrain);
            c.erosion_mask = i % 2 == 0;
            c.erosion = 0.125;
            c.sediment = 0.25;
            c.gradient = 0.5;
            c.moisture = 0.75;
            c.temperature = 0.375;
            c.continent_id = 0.625;
            c.continent_edge = 0.4;
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
        fn applies_every_cell_field_like_engine() {
            let registry = TerrainRegistry::new();
            let mut fold = Fold(17);
            let mut applies = 0;
            for levels in levels() {
                for config in configs(levels) {
                    fold.f(config.depth);
                    fold.f(config.bank_min);
                    fold.f(config.bank_max);
                    let mut all = Boundsf::builder();
                    for [cx, cz] in CENTERS {
                        for [radius, multiplier] in SIZES {
                            let k = LakeCarver::new(
                                Vec2f::new(cx, cz),
                                radius,
                                multiplier,
                                config,
                                &registry,
                            );
                            fold.lake(&k);
                            let mut one = Boundsf::builder();
                            k.record_bounds(&mut one);
                            k.record_bounds(&mut all);
                            let bounds = one.build();
                            fold.bounds(bounds);
                            for off in OFFS {
                                for [x, z] in [
                                    [cx + off, cz],
                                    [cx, cz - off],
                                    [cx + off * 0.6, cz + off * 0.8],
                                ] {
                                    for i in std::iter::once(None).chain((0..FILLS.len()).map(Some))
                                    {
                                        let mut c = fill(i, &registry);
                                        k.apply(&mut c, x, z);
                                        fold.cell(&c, &registry);
                                        applies += 1;
                                    }
                                    for radius2 in RADII2 {
                                        fold.b(k.overlaps(x, z, radius2));
                                    }
                                    fold.b(bounds.contains(x, z));
                                    fold.b(Boundsf::NONE.contains(x, z));
                                }
                            }
                        }
                    }
                    fold.bounds(all.build());
                }
            }
            assert_eq!((applies, fold.0), (561_600, -511_542_011));
        }

        #[test]
        fn builds_bounds_like_engine() {
            let mut fold = Fold(17);
            fold.bounds(Boundsf::builder().build());
            fold.bounds(Boundsf::NONE);
            let records = [
                [-0.0, 0.0],
                [0.0, -0.0],
                [1.0, -1.0],
                [NAN, 2.0],
                [3.0, NAN],
                [-INF, INF],
                [1e-45, -1e-45],
            ];
            for start in 0..records.len() {
                let mut builder = Boundsf::builder();
                for [x, y] in &records[start..] {
                    builder.record(*x, *y);
                    fold.bounds(builder.build());
                }
            }
            assert_eq!(fold.0, 434_349_810);
            // `Float.MIN_VALUE`, not `f32::MIN_POSITIVE`.
            assert_eq!(Boundsf::builder().build().max_x.to_bits(), 1);
        }

        #[test]
        fn samples_match_engine() {
            let registry = TerrainRegistry::new();
            let levels = levels()[0];
            let lake = LakeCarver::new(
                Vec2f::new(100.0, -50.0),
                75.0,
                0.8,
                LakeConfig::new(Lake::default(), levels),
                &registry,
            );
            assert_eq!(
                [100.0, 130.0, 160.0, 175.0, 200.0, 300.0, 321.0].map(|x| {
                    let mut c = fill(Some(0), &registry);
                    lake.apply(&mut c, x, -50.0);
                    (c.value.to_bits(), c.river_mask.to_bits(), c.terrain.id())
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
        }
    }

    /// Engine 0.3.0's `Wetland` against `WetlandOracle.java` (`.local/terraforged/oracles/river/`,
    /// `run-wetland.sh`), which runs Engine's class on the same inputs.
    mod wetland_oracle {
        use super::lake_oracle::{Fold, fill_from, levels};
        use super::*;

        const NAN: f32 = f32::NAN;
        const INF: f32 = f32::INFINITY;
        const SEEDS: [i32; 3] = [12_345, i32::MAX - 1, -1];
        /// a.x, a.y, b.x, b.y.
        #[rustfmt::skip]
        const SEGMENTS: [[f32; 4]; 6] = [
            [0.0, 0.0, 500.0, 200.0], [-0.0, -0.0, -0.0, -0.0], [1e7, -3.5e6, 1e7 + 300.0, -3.5e6],
            [-120.0, 40.0, -120.0, -260.0], [NAN, 0.0, 10.0, 10.0], [0.0, 0.0, INF, 0.0],
        ];
        const RADII: [f32; 7] = [100.0, 150.0, 0.0, -75.0, NAN, INF, 1e20];
        const TS: [f32; 5] = [-0.25, 0.0, 0.5, 1.0, 1.25];
        #[rustfmt::skip]
        const OFFS: [f32; 17] = [
            0.0, -0.0, 1.0, 20.0, 50.0, 59.160_797, 59.999_996, 60.0, 70.0, 99.999_99, 100.0, 100.000_01,
            150.0, 1e20, NAN, INF, -INF,
        ];
        /// value, river mask, terrain id (erosion mask: even fills); for `Levels(256, 63)` the bed is
        /// 0.236328125, the mound top 0.25 and the banks 0.2578125.
        #[rustfmt::skip]
        pub(super) const FILLS: [(f32, f32, i32); 14] = [
            (0.6, 1.0, 0), (0.3, 0.5, 7), (0.2, NAN, 5), (0.0, -0.0, 6), (1.0, 1.0, 12), (NAN, 1.0, 0),
            (0.24, 0.75, 8), (0.25, 1.0, 7), (-0.5, 1.0, 11), (-0.0, 2.0, 0), (INF, -1.0, 0),
            (0.242_187_5, 0.25, 3), (0.236_328_125, 0.5, 0), (0.257_812_5, 1.0, 10),
        ];

        fn mix_wetland(fold: &mut Fold, w: &WetlandCarver) {
            for v in [
                w.radius,
                w.radius2,
                w.bed,
                w.banks,
                w.mound_min,
                w.mound_max,
                w.mound_variance,
                w.start.x,
                w.start.y,
                w.end.x,
                w.end.y,
            ] {
                fold.f(v);
            }
        }

        #[test]
        fn applies_every_cell_field_like_engine() {
            let registry = TerrainRegistry::new();
            let fill = |i| fill_from(&FILLS, i, &registry);
            let mut fold = Fold(17);
            let mut applies = 0;
            for levels in levels() {
                for seed in SEEDS {
                    let mut all = Boundsf::builder();
                    for [ax, ay, bx, by] in SEGMENTS {
                        for radius in RADII {
                            let w = WetlandCarver::new(
                                seed,
                                Vec2f::new(ax, ay),
                                Vec2f::new(bx, by),
                                radius,
                                levels,
                                &registry,
                            );
                            mix_wetland(&mut fold, &w);
                            let mut one = Boundsf::builder();
                            w.record_bounds(&mut one);
                            w.record_bounds(&mut all);
                            fold.bounds(one.build());
                            for t in TS {
                                for off in OFFS {
                                    let (px, pz) = (ax + t * (bx - ax), ay + t * (by - ay));
                                    for [x, z] in [[px, pz + off], [px + off * 0.6, pz - off * 0.8]]
                                    {
                                        for i in
                                            std::iter::once(None).chain((0..FILLS.len()).map(Some))
                                        {
                                            let mut c = fill(i);
                                            w.apply(&mut c, x, z);
                                            fold.cell(&c, &registry);
                                            let mut c = fill(i);
                                            w.apply_warped(
                                                &mut c,
                                                x,
                                                z,
                                                x * 0.5 + 1000.5,
                                                z - 777.25,
                                            );
                                            fold.cell(&c, &registry);
                                            applies += 2;
                                        }
                                    }
                                }
                            }
                        }
                    }
                    fold.bounds(all.build());
                }
            }
            assert_eq!((applies, fold.0), (1_927_800, -1_805_994_020));
        }

        #[test]
        fn samples_match_engine() {
            let registry = TerrainRegistry::new();
            let wetland = WetlandCarver::new(
                12_345,
                Vec2f::new(0.0, 0.0),
                Vec2f::new(500.0, 200.0),
                100.0,
                Levels::new(256, 63),
                &registry,
            );
            let sample = |[river_x, river_z, x, z, value]: [f32; 5]| {
                let mut cell = Cell::new(&registry);
                cell.value = value;
                cell.river_mask = 1.0;
                wetland.apply_warped(&mut cell, river_x, river_z, x, z);
                assert!(cell.terrain.is_same(&registry.get_by_id(cell.terrain.id())));
                (
                    cell.value.to_bits(),
                    cell.river_mask.to_bits(),
                    cell.erosion_mask,
                    cell.terrain.id(),
                )
            };
            assert_eq!(
                [
                    [0.0, 0.0, 0.0, 0.0, 0.6],
                    [250.0, 100.0, 250.0, 100.0, 0.6],
                    [250.0, 150.0, 250.0, 150.0, 0.6],
                    [250.0, 190.0, 250.0, 190.0, 0.6],
                    [250.0, 210.0, 250.0, 210.0, 0.6],
                    [550.0, 220.0, 550.0, 220.0, 0.6],
                    [250.0, 100.0, 1000.0, -1000.0, 0.6],
                    [250.0, 100.0, 250.0, 100.0, 0.1],
                    [250.0, 160.0, 250.0, 160.0, 0.24],
                    [250.0, 168.0, 250.0, 168.0, 0.25],
                ]
                .map(sample),
                [
                    (0x3e7b5555, 0, true, 7),
                    (0x3e7b5555, 0, true, 7),
                    (0x3e7b5555, 0, true, 7),
                    (0x3ee1dfaf, 0x3f092ac9, false, 0),
                    (0x3f19999a, 0x3f800000, false, 0),
                    (0x3e7b5555, 0, true, 7),
                    (0x3e7b5555, 0, true, 7),
                    (0x3dcccccd, 0x3f800000, false, 0),
                    (0x3e79c4d7, 0, false, 7),
                    (0x3e800000, 0x3d993150, false, 0),
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
    }

    /// Engine 0.3.0's `Network` and `Network.Builder` against `NetworkOracle.java` (in the maintainer's
    /// `.local/terraforged/oracles/river/`), which runs Engine's classes with real carvers, lakes and wetlands.
    mod network_oracle {
        use super::lake_oracle::{Fold, fill_from, levels};
        use super::*;

        const NAN: f32 = f32::NAN;
        const INF: f32 = f32::INFINITY;
        /// `WetlandOracle`'s fills: value, river mask, terrain id (erosion mask: even fills).
        #[rustfmt::skip]
        pub(super) const FILLS: [(f32, f32, i32); 14] = [
            (0.6, 1.0, 0), (0.3, 0.5, 7), (0.2, NAN, 5), (0.0, -0.0, 6), (1.0, 1.0, 12), (NAN, 1.0, 0),
            (0.24, 0.75, 8), (0.25, 1.0, 7), (-0.5, 1.0, 11), (-0.0, 2.0, 0), (INF, -1.0, 0),
            (0.242_187_5, 0.25, 3), (0.236_328_125, 0.5, 0), (0.257_812_5, 1.0, 10),
        ];
        #[rustfmt::skip]
        const PROBES: [[f32; 4]; 11] = [
            [500.0, -100.0, 500.0, 100.0], [2000.0, 2000.0, 2100.0, 2100.0], [-100.0, 0.0, 100.0, 0.0],
            [1050.0, 10.0, 1300.0, 10.0], [510.0, 300.0, 900.0, 300.0], [-0.0, -0.0, -0.0, -0.0],
            [0.0, NAN, 10.0, 10.0], [-INF, 300.0, INF, 300.0], [-200.0, 200.0, 0.0, 200.0],
            [500.0, -520.0, 480.0, -700.0], [5.0, 5.0, 5.0, 20.0],
        ];
        const EXTENDS: [f32; 6] = [0.0, 50.0, 250.0, -10.0, NAN, INF];

        fn carver(
            [x1, z1, x2, z2]: [f32; 4],
            main: bool,
            warp: RiverWarp,
            levels: Levels,
            registry: &TerrainRegistry,
        ) -> RiverCarver {
            let mut config = RiverConfig::builder(levels)
                .main(main)
                .fade(0.75)
                .length(1000);
            if main {
                config = config
                    .bed_width(8)
                    .bank_width(20)
                    .bed_depth(5)
                    .bank_height(2, 6);
            }
            RiverCarver::new(
                River::new(x1, z1, x2, z2),
                warp,
                config.build(),
                RiverCarverSettings::default(),
                levels,
                registry,
            )
        }

        fn mix_builder(fold: &mut Fold, b: &RiverNetworkBuilder) {
            for v in [b._min_x, b._min_z, b._max_x, b._max_z] {
                fold.f(v);
            }
        }

        fn mix_bounds(fold: &mut Fold, n: &RiverNetwork) {
            fold.bounds(n.bounds);
            for child in &n.children {
                mix_bounds(fold, child);
            }
        }

        /// `NetworkOracle.roots`: networks a, b and c.
        pub(super) fn roots(
            fold: &mut Fold,
            levels: Levels,
            registry: &TerrainRegistry,
        ) -> [RiverNetworkBuilder; 3] {
            let lakes = LakeConfig::new(Lake::default(), levels);
            let lake = |x, z, radius, multiplier| {
                LakeCarver::new(Vec2f::new(x, z), radius, multiplier, lakes, registry)
            };
            let wetland = |seed, [ax, az, bx, bz]: [f32; 4], radius| {
                WetlandCarver::new(
                    seed,
                    Vec2f::new(ax, az),
                    Vec2f::new(bx, bz),
                    radius,
                    levels,
                    registry,
                )
            };
            let carver = |line, main, warp| carver(line, main, warp, levels, registry);
            let mut a = RiverNetwork::builder(carver(
                [0.0, 0.0, 1000.0, 0.0],
                true,
                RiverWarp::new(12_345, 0.15, 0.85, 0.001, 150.0),
            ));
            a.lakes.push(lake(200.0, 100.0, 75.0, 0.8));
            a.lakes.push(lake(0.0, 0.0, 40.0, 1.0));
            a.wetlands
                .push(wetland(12_345, [600.0, -100.0, 900.0, -100.0], 80.0));
            let mut a1 = RiverNetwork::builder(carver(
                [500.0, -500.0, 500.0, 500.0],
                false,
                RiverWarp::new(-7, 0.15, 0.75, 0.0015, 90.0),
            ));
            a1.lakes.push(lake(500.0, -500.0, 50.0, 1.0));
            let mut a11 = RiverNetwork::builder(carver(
                [500.0, 200.0, 800.0, 450.0],
                false,
                RiverWarp::none(),
            ));
            a11.wetlands
                .push(wetland(-1, [520.0, 250.0, 700.0, 400.0], 60.0));
            mix_builder(fold, &a11);
            a11.add_bounds(River::new(-5000.0, 0.0, 0.0, 9000.0));
            mix_builder(fold, &a11);
            a1.children.push(a11);
            a.children.push(a1);
            let a2 = RiverNetwork::builder(carver(
                [900.0, 0.0, 1200.0, -300.0],
                false,
                RiverWarp::new(0, 0.0, 0.0, 0.0, 0.0),
            ));
            a.children.push(a2);
            let mut b = RiverNetwork::builder(carver(
                [100.0, -50.0, -300.0, 400.0],
                true,
                RiverWarp::new(i32::MAX, 0.3, 0.7, 0.002, 60.0),
            ));
            b.wetlands
                .push(wetland(i32::MAX - 1, [-300.0, 0.0, 200.0, 300.0], 100.0));
            b.wetlands
                .push(wetland(77, [-250.0, 300.0, 150.0, -20.0], 90.0));
            b.lakes.push(lake(-50.0, 150.0, 60.0, 1.0));
            b.lakes.push(lake(0.0, 120.0, 90.0, 0.5));
            let mut c =
                RiverNetwork::builder(carver([5.0, 5.0, 5.0, 5.0], false, RiverWarp::none()));
            c.lakes.push(lake(NAN, 0.0, 30.0, 1.0));
            c.wetlands.push(wetland(3, [0.0, 0.0, INF, 0.0], 50.0));
            c.children.push(RiverNetwork::builder(carver(
                [0.0, NAN, 10.0, 10.0],
                false,
                RiverWarp::new(5, 0.2, 0.8, 0.001, 100.0),
            )));
            c.add_bounds(River::new(NAN, -3.0, 2.0, NAN));
            let roots = [a, b, c];
            for builder in all_builders(&roots) {
                mix_builder(fold, builder);
            }
            roots
        }

        /// The oracle's `builders` list: a, a1, a11, a2, b, c, c1.
        fn all_builders(roots: &[RiverNetworkBuilder; 3]) -> [&RiverNetworkBuilder; 7] {
            let [a, b, c] = roots;
            let a1 = &a.children[0];
            [a, a1, &a1.children[0], &a.children[1], b, c, &c.children[0]]
        }

        #[test]
        fn carves_every_cell_field_like_engine() {
            let registry = TerrainRegistry::new();
            let mut fold = Fold(17);
            let mut carves = 0;
            let probes = PROBES.map(|[x1, z1, x2, z2]| River::new(x1, z1, x2, z2));
            let mut points = Vec::new();
            for i in 0..=44 {
                for j in 0..=28 {
                    points.push((-600.0 + 50.0 * i as f32, -700.0 + 50.0 * j as f32));
                }
            }
            for e in [-0.0, NAN, INF, -INF, 1e20] {
                points.push((e, 0.0));
                points.push((500.0, e));
            }
            for levels in levels() {
                let roots = roots(&mut fold, levels, &registry);
                let builders = all_builders(&roots);
                for builder in builders {
                    for probe in probes {
                        for extend in EXTENDS {
                            fold.b(builder.overlaps(probe, None, extend));
                            for parent in builders {
                                fold.b(builder.overlaps(probe, Some(parent), extend));
                            }
                        }
                    }
                }
                let networks = roots.map(RiverNetworkBuilder::build);
                for network in &networks {
                    mix_bounds(&mut fold, network);
                    for probe in probes {
                        for extend in EXTENDS {
                            fold.b(network.overlaps(probe, extend));
                        }
                    }
                }
                for &(x, z) in &points {
                    for network in &networks {
                        fold.b(network.contains(x, z));
                        for (lake_x, lake_z) in [(0.0, 0.0), (20.0, -10.0), (NAN, -0.0)] {
                            for i in std::iter::once(None).chain((0..FILLS.len()).map(Some)) {
                                let mut cell = fill_from(&FILLS, i, &registry);
                                network.carve(&mut cell, x, z, lake_x, lake_z);
                                fold.cell(&cell, &registry);
                                carves += 1;
                            }
                        }
                    }
                }
            }
            assert_eq!((carves, fold.0), (532_575, 362_163_377));
        }

        #[test]
        fn samples_match_engine() {
            let registry = TerrainRegistry::new();
            let [a, b, _] = roots(&mut Fold(0), levels()[0], &registry);
            let (a, b) = (a.build(), b.build());
            #[rustfmt::skip]
            let samples = [
                (&a, 500.0, 0.0, 0.0, 0.0), (&a, 200.0, 100.0, 0.0, 0.0), (&a, 750.0, -100.0, 0.0, 0.0),
                (&a, 500.0, 100.0, 20.0, -10.0), (&a, 1100.0, 0.0, 0.0, 0.0), (&a, 520.0, 300.0, 0.0, 0.0),
                (&a, 650.0, 330.0, 0.0, 0.0), (&a, 500.0, -480.0, 0.0, 0.0), (&a, 300.0, 40.0, 0.0, 0.0),
                (&a, 2000.0, 2000.0, 0.0, 0.0), (&b, -30.0, 130.0, 0.0, 0.0), (&b, -10.0, 140.0, 5.0, 5.0),
                (&b, -100.0, 170.0, 0.0, 0.0),
            ];
            let carved = samples.map(|(network, x, z, lake_x, lake_z)| {
                let mut cell = Cell::new(&registry);
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
            });
            #[rustfmt::skip]
            assert_eq!(
                carved,
                [
                    (0x3e7c_0000, 0, false, 0), (0x3e80_7b26, 0x3954_37dc, false, 0), (0x3e7e_1cab, 0, true, 7),
                    (0x3e7c_0000, 0, false, 0), (0x3e8f_6356, 0x3e4b_f690, false, 0), (0x3e7f_f56e, 0, true, 7),
                    (0x3e81_97b8, 0x3d38_2050, false, 0), (0x3e50_2f12, 0x3958_f000, false, 6),
                    (0x3e6f_9e2e, 0x3920_e000, true, 5), (0x3f19_999a, 0x3f80_0000, false, 0),
                    (0x3e7c_a476, 0, true, 7), (0x3e60_b4ba, 0, true, 6), (0x3e7e_dcfe, 0, true, 7),
                ]
            );
            let bounds = a.bounds;
            assert_eq!(
                [bounds.min_x, bounds.min_y, bounds.max_x, bounds.max_y].map(f32::to_bits),
                [0xc409_8000, 0xc483_4000, 0x44da_c000, 0x4483_4000]
            );
        }
    }

    /// Engine 0.3.0's `Rivermap` against `RivermapOracle.java` (in the maintainer's
    /// `.local/terraforged/oracles/river/`), which runs Engine's class over `NetworkOracle`'s networks.
    mod rivermap_oracle {
        use super::lake_oracle::{Fold, fill_from, levels};
        use super::network_oracle::{FILLS, roots};
        use super::*;

        fn warps() -> [GenWarp; 4] {
            [
                GenWarp::new(12_345, 3_000),
                GenWarp::new(i32::MAX - 2, 0),
                GenWarp::new(-1, 500),
                GenWarp::direct(),
            ]
        }

        #[test]
        fn applies_every_cell_field_like_engine() {
            let registry = TerrainRegistry::new();
            let mut fold = Fold(17);
            let mut applies = 0;
            let mut points = Vec::new();
            for i in 0..=40 {
                for j in 0..=40 {
                    points.push((-700.0 + 60.0 * i as f32, -800.0 + 40.0 * j as f32));
                }
            }
            for e in [-0.0, f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 1e20] {
                points.push((e, 0.0));
                points.push((500.0, e));
            }
            for levels in levels() {
                let networks = roots(&mut fold, levels, &registry).map(RiverNetworkBuilder::build);
                let mut networks = Vec::from(networks);
                for (w, warp) in (0..).zip(warps()) {
                    let map = Rivermap::new(-3 * w, w - 7, networks, warp);
                    fold.mix(map.x());
                    fold.mix(map.z());
                    for &(x, z) in &points {
                        for i in std::iter::once(None).chain((0..FILLS.len()).map(Some)) {
                            let mut cell = fill_from(&FILLS, i, &registry);
                            map.apply(&mut cell, x, z);
                            fold.cell(&cell, &registry);
                            applies += 1;
                        }
                    }
                    networks = map.networks;
                }
            }
            assert_eq!((applies, fold.0), (304_380, -811_895_622));
        }

        #[test]
        fn samples_match_engine() {
            let registry = TerrainRegistry::new();
            let networks =
                roots(&mut Fold(0), levels()[0], &registry).map(RiverNetworkBuilder::build);
            let [warp, ..] = warps();
            let map = Rivermap::new(17, -9, Vec::from(networks), warp);
            assert_eq!((map.x(), map.z()), (17, -9));
            assert!(map.timestamp() > 0);
            // Engine's rivermap cache hands one map to every generation thread.
            fn shared<T: Send + Sync>(_: &T) {}
            shared(&map);
            #[rustfmt::skip]
            let samples = [
                (0.0, 0.0), (500.0, 0.0), (500.0, 50.0), (1000.0, 0.0), (200.0, 100.0), (750.0, -100.0),
                (-50.0, 150.0), (520.0, 300.0), (500.0, -480.0), (2000.0, 2000.0),
            ];
            let applied = samples.map(|(x, z)| {
                let mut cell = Cell::new(&registry);
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
            });
            #[rustfmt::skip]
            assert_eq!(
                applied,
                [
                    (0x3e50_0000, 0, true, 6), (0x3e7c_0083, 0x38f8_4000, false, 0),
                    (0x3e7c_0490, 0x3a8f_4800, false, 0), (0x3e69_0fd5, 0x3a78_b000, true, 5),
                    (0x3e80_d2df, 0x3a7e_a30b, false, 0), (0x3e7e_137b, 0, true, 7),
                    (0x3e7b_7aa7, 0, true, 7), (0x3e75_392d, 0, true, 7),
                    (0x3e73_cd30, 0x39a7_a000, false, 6), (0x3f19_999a, 0x3f80_0000, false, 0),
                ]
            );
        }
    }

    /// Engine 0.3.0's `RiverCache` and `LegacyRiverCache` against `RiverCacheOracle.java` (in the maintainer's
    /// `.local/terraforged/oracles/river/`): the (x, z, id) each passes its generator, one generation per cell, and the
    /// cached instance on the second lookup.
    #[test]
    fn river_caches_match_engine_0_3_0() {
        #[rustfmt::skip]
        let expected: [(i32, i32, u64, u64); 6] = [
            (-7, 9, 0xfffffff9_00000009, 0x00000009_fffffff9),
            (0, 0, 0, 0),
            (17, -9, 0x00000011_fffffff7, 0xfffffff7_00000011),
            (i32::MIN, i32::MAX, 0x80000000_7fffffff, 0x7fffffff_80000000),
            (-1, -1, u64::MAX, u64::MAX),
            (3, i32::MIN, 0x00000003_80000000, 0x80000000_00000003),
        ];
        let cache = RiverCache::new();
        let legacy = LegacyRiverCache::new();
        for (x, z, id, legacy_id) in expected {
            for (is_legacy, expected_id) in [(false, id), (true, legacy_id)] {
                let mut calls = Vec::new();
                let mut lookup = || {
                    let generate = |gx, gz, gid: i64| {
                        calls.push((gx, gz, gid as u64));
                        Rivermap::new(gx, gz, Vec::new(), GenWarp::direct())
                    };
                    if is_legacy {
                        legacy.get_rivers(x, z, generate)
                    } else {
                        cache.get_rivers(x, z, generate)
                    }
                };
                let first = lookup();
                let second = lookup();
                assert!(Arc::ptr_eq(&first, &second));
                assert_eq!((first.x(), first.z()), (x, z));
                assert_eq!(calls, [(x, z, expected_id)]);
            }
        }
    }

    /// The sweep drops maps older than five minutes (on a miss, at most once a minute) and keeps the rest; a dropped
    /// map is generated again on its next lookup.
    #[test]
    fn river_cache_expires_old_maps() {
        let cache = RiverCache::new();
        let make = |x, z, age: i64| {
            let mut map = Rivermap::new(x, z, Vec::new(), GenWarp::direct());
            map.timestamp -= age;
            map
        };
        let old = cache.get_rivers(1, 1, |x, z, _| {
            make(x, z, RiverCache::LIFETIME_MILLIS + 1_000)
        });
        // The first miss swept the empty cache; within the minute, the next one does not sweep.
        let fresh = cache.get_rivers(2, 2, |x, z, _| make(x, z, 0));
        assert!(Arc::ptr_eq(
            &old,
            &cache.get_rivers(1, 1, |_, _, _| unreachable!())
        ));
        cache.last_sweep.store(0, Ordering::Relaxed);
        cache.get_rivers(3, 3, |x, z, _| make(x, z, 0));
        assert_eq!(cache.entries.read().unwrap().len(), 2);
        assert!(Arc::ptr_eq(
            &fresh,
            &cache.get_rivers(2, 2, |_, _, _| unreachable!())
        ));
        let again = cache.get_rivers(1, 1, |x, z, _| make(x, z, 0));
        assert!(!Arc::ptr_eq(&old, &again));
        assert_eq!((again.x(), again.z()), (1, 1));
    }

    /// The caches and the fancy continent generator that owns one are shared by worldgen threads.
    #[test]
    fn river_caches_are_send_and_sync() {
        fn shared<T: Send + Sync>() {}
        shared::<RiverCache>();
        shared::<LegacyRiverCache>();
        shared::<super::super::terraforged_fancy::FancyContinentGenerator>();
    }

    /// Engine 0.3.0's `BaseRiverGenerator` against `BaseRiverGeneratorOracle.java` (in the maintainer's
    /// `.local/terraforged/oracles/river/`), which runs Engine's class through a subclass with radial roots.
    mod base_river_generator_oracle {
        use super::lake_oracle::{Fold, fill_from};
        use super::network_oracle::FILLS;
        use super::*;
        use std::sync::Arc;

        struct TestContinent(fn(f32, f32) -> f32);

        impl ContinentSampler for TestContinent {
            fn edge_value(&self, x: f32, z: f32) -> f32 {
                (self.0)(x, z)
            }
        }

        const EDGES: [fn(f32, f32) -> f32; 4] = [
            |_, _| 1.0,
            |x, z| 1.0 - f64::from(x * x + z * z).sqrt() as f32 / 4000.0,
            |_, _| f32::NAN,
            |_, _| 0.0,
        ];

        /// The oracle's `generateRoots`: radial main rivers, every third one from `createFork`, each with a lake.
        fn roots(
            generator: &BaseRiverGenerator<TestContinent>,
            x: i32,
            z: i32,
            random: &mut LegacyRandom,
            warp: &GenWarp,
        ) -> Vec<RiverNetworkBuilder> {
            let mut roots = Vec::new();
            let start = random.next_float();
            for i in 0..generator.count {
                let angle =
                    start + PI2 * i as f32 / generator.count as f32 + random.next_float() * 0.3;
                let dx = sin(angle);
                let dz = cos(angle);
                let length = 1500.0 + random.next_float() * 2500.0;
                let (x1, z1) = (x as f32 + dx * 300.0, z as f32 + dz * 300.0);
                let (x2, z2) = (x as f32 + dx * length, z as f32 + dz * length);
                let carver = if i % 3 == 2 {
                    generator.create_fork(x1, z1, x2, z2, 200.0, generator.fork, random)
                } else {
                    generator.create_main(x1, z1, x2, z2, generator.main, random)
                };
                let mut builder = RiverNetwork::builder(carver);
                generator.add_lake(&mut builder, random, warp);
                roots.push(builder);
            }
            roots
        }

        fn settings(variant: i32) -> (WorldSettings, RiverSettings) {
            let mut world = WorldSettings {
                seed: 12_345,
                ..WorldSettings::default()
            };
            let mut rivers = RiverSettings::default();
            match variant {
                1 => {
                    world.seed = -987_654_321;
                    rivers.seed_offset = i32::MAX;
                    rivers.lakes.chance = 1.0;
                    rivers.wetlands.chance = 1.0;
                    rivers.river_count = 3;
                }
                2 => {
                    world.properties.world_height = 384;
                    world.properties.sea_level = 62;
                    rivers.lakes.chance = 0.0;
                    world.control_points.inland = 0.75;
                }
                3 => {
                    rivers.lakes.chance = 0.5;
                    rivers.wetlands.chance = 0.95;
                    rivers.river_count = 5;
                    world.continent.continent_scale = 1000;
                }
                _ => {}
            }
            (world, rivers)
        }

        fn generator(
            variant: i32,
            edge: fn(f32, f32) -> f32,
            registry: &Arc<TerrainRegistry>,
        ) -> BaseRiverGenerator<TestContinent> {
            let (world, rivers) = settings(variant);
            // `GeneratorContext`: `Seed((int) world.seed)` and `Levels(world)`.
            let levels = Levels::new(world.properties.world_height, world.properties.sea_level);
            BaseRiverGenerator::new(
                TestContinent(edge),
                world.seed as i32,
                &world,
                &rivers,
                levels,
                Arc::clone(registry),
            )
        }

        fn mix_range(fold: &mut Fold, range: Range) {
            fold.f(range.min);
            fold.f(range.max);
        }

        fn mix_lake(fold: &mut Fold, lake: &LakeCarver) {
            for v in [
                lake.valley,
                lake.lake_distance2,
                lake.depth,
                lake.bank_min,
                lake.bank_max,
                lake.center.x,
                lake.center.y,
            ] {
                fold.f(v);
            }
        }

        pub(super) fn mix_network(fold: &mut Fold, network: &RiverNetwork) -> i32 {
            let carver = &network.river_carver;
            let river = carver.river;
            for v in [river.x1, river.z1, river.x2, river.z2] {
                fold.f(v);
            }
            fold.b(carver.main);
            fold.b(carver._connecting);
            fold.f(carver.fade);
            for range in [
                carver.bed_width,
                carver.banks_width,
                carver.valley_width,
                carver.bed_depth,
                carver.banks_depth,
            ] {
                mix_range(fold, range);
            }
            let config = carver.config;
            fold.mix(config.order);
            fold.b(config.main);
            fold.mix(config.bed_width);
            fold.mix(config.bank_width);
            fold.f(config.bed_height);
            fold.f(config.min_bank_height);
            fold.f(config.max_bank_height);
            fold.mix(config.length);
            fold.f(config.length2 as f32);
            fold.f(config.fade);
            let warp = carver.warp;
            fold.b(!warp.enabled);
            for v in [
                warp.lower,
                warp.upper,
                warp.lower_range,
                warp.upper_range,
                warp.frequency,
                warp.scale,
            ] {
                fold.f(v);
            }
            fold.mix(warp.seed);
            fold.mix(network.lakes.len() as i32);
            for lake in &network.lakes {
                mix_lake(fold, lake);
            }
            fold.mix(network.wetlands.len() as i32);
            for wetland in &network.wetlands {
                for v in [
                    wetland.start.x,
                    wetland.start.y,
                    wetland.end.x,
                    wetland.end.y,
                    wetland.radius,
                    wetland.bed,
                    wetland.banks,
                    wetland.mound_min,
                    wetland.mound_max,
                    wetland.mound_shape.value(100.0, 200.0),
                ] {
                    fold.f(v);
                }
            }
            fold.mix(network.children.len() as i32);
            let mut nodes = 1;
            for child in &network.children {
                nodes += mix_network(fold, child);
            }
            fold.bounds(network.bounds);
            nodes
        }

        #[test]
        fn generates_networks_like_engine() {
            let registry = Arc::new(TerrainRegistry::new());
            let ids = [0x1234_5678, -1, 0x7fff_ffff_0000_0005, 0];
            let mut fold = Fold(17);
            let (mut applies, mut networks, mut nodes) = (0, 0, 0);
            for variant in 0..4 {
                for edge in EDGES {
                    let generator = generator(variant, edge, &registry);
                    fold.mix(generator.count);
                    fold.mix(generator.continent_scale);
                    fold.f(generator.min_edge_value);
                    fold.mix(generator.seed);
                    for (i, id) in (0..).zip(ids) {
                        let (x, z) = (200 * i - 300, 150 - 100 * i);
                        let map = generator.generate_rivers(x, z, id, roots);
                        fold.mix(map.x());
                        fold.mix(map.z());
                        fold.mix(map.networks.len() as i32);
                        networks += map.networks.len();
                        for network in &map.networks {
                            nodes += mix_network(&mut fold, network);
                        }
                        for px in 0..=50 {
                            for pz in 0..=50 {
                                let px = -4500.0 + 180.0 * px as f32 + x as f32;
                                let pz = -4500.0 + 180.0 * pz as f32 + z as f32;
                                for fill in [None, Some(0), Some(1)] {
                                    let mut cell = fill_from(&FILLS, fill, &registry);
                                    map.apply(&mut cell, px, pz);
                                    fold.cell(&cell, &registry);
                                    applies += 1;
                                }
                            }
                        }
                    }
                }
            }
            assert_eq!(
                (applies, networks, nodes, fold.0),
                (499_392, 384, 1157, -904_012_376)
            );
        }

        #[test]
        fn extra_lakes_and_overlaps_match_engine() {
            let registry = Arc::new(TerrainRegistry::new());
            let generator = generator(0, EDGES[0], &registry);
            let mut random = LegacyRandom::new(42);
            let warp = GenWarp::new(42, 3000);
            let roots = roots(&generator, 30, -40, &mut random, &warp);
            let carvers: Vec<RiverCarver> = roots
                .iter()
                .map(|root| {
                    let c = &root.carver;
                    RiverCarver::new(
                        c.river,
                        c.warp,
                        c.config,
                        RiverCarverSettings::default(),
                        generator.levels,
                        &registry,
                    )
                })
                .collect();
            let mut lakes = Vec::new();
            generator.generate_additional_lakes(30, -40, &mut random, &roots, &carvers, &mut lakes);
            let mut fold = Fold(17);
            fold.mix(lakes.len() as i32);
            for lake in &lakes {
                mix_lake(&mut fold, lake);
            }
            fold.mix(random.next_int());
            #[rustfmt::skip]
            let probes = [
                [0.0, 0.0, 1000.0, 0.0], [5000.0, 5000.0, 5100.0, 5100.0], [-300.0, -300.0, 300.0, 300.0],
                [2000.0, -2000.0, 2000.0, 2000.0], [f32::NAN, 0.0, 10.0, 10.0],
            ];
            type Generator = BaseRiverGenerator<TestContinent>;
            for [x1, z1, x2, z2] in probes {
                let river = River::new(x1, z1, x2, z2);
                fold.b(Generator::river_overlaps(river, None, &roots));
                fold.b(Generator::river_overlaps(river, Some(&roots[0]), &roots));
                fold.b(Generator::lake_overlaps(
                    Vec2f::new(x1, z1),
                    150.0,
                    &carvers,
                ));
                fold.b(Generator::lake_overlaps_other(x1, z1, 100.0, &lakes));
            }
            assert_eq!((lakes.len(), fold.0), (7, 106_237_297));
        }

        #[test]
        fn samples_match_engine() {
            let registry = Arc::new(TerrainRegistry::new());
            let generator = generator(0, EDGES[0], &registry);
            let map = generator.generate_rivers(17, -9, 0x1234_5678, roots);
            #[rustfmt::skip]
            let samples = [
                (17.0, -9.0), (500.0, 0.0), (0.0, 1000.0), (-1500.0, 300.0), (1200.0, -1800.0),
                (-800.0, -2600.0), (2500.0, 2500.0), (3000.0, -200.0), (-3500.0, -100.0), (100.0, 3200.0),
            ];
            let applied = samples.map(|(x, z)| {
                let mut cell = Cell::new(&registry);
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
            });
            #[rustfmt::skip]
            assert_eq!(
                applied,
                [
                    (0x3f17_7bf9, 0x3f79_f47d, false, 0), (0x3e84_105d, 0x3c9c_1260, false, 0),
                    (0x3e6b_306c, 0x3a0e_9800, true, 5), (0x3f19_999a, 0x3f80_0000, false, 0),
                    (0x3eb4_0d05, 0x3e92_e3fa, false, 0), (0x3f19_999a, 0x3f80_0000, false, 0),
                    (0x3f19_999a, 0x3f80_0000, false, 0), (0x3f19_999a, 0x3f80_0000, false, 0),
                    (0x3f19_999a, 0x3f80_0000, false, 0), (0x3f19_999a, 0x3f80_0000, false, 0),
                ]
            );
        }
    }

    mod fancy_river_generator_oracle {
        use super::super::super::terraforged_engine::Seed;
        use super::super::super::terraforged_fancy::FancyContinentGenerator;
        use super::base_river_generator_oracle::mix_network;
        use super::lake_oracle::{Fold, fill_from};
        use super::network_oracle::FILLS;
        use super::*;
        use std::sync::Arc;

        fn settings(variant: i32) -> (WorldSettings, RiverSettings) {
            let mut world = WorldSettings {
                seed: 12_345,
                ..WorldSettings::default()
            };
            let mut rivers = RiverSettings::default();
            match variant {
                1 => {
                    world.seed = -987_654_321;
                    rivers.seed_offset = i32::MAX;
                    rivers.lakes.chance = 1.0;
                    rivers.wetlands.chance = 1.0;
                }
                2 => {
                    world.properties.world_height = 384;
                    world.properties.sea_level = 62;
                    world.control_points.inland = 0.75;
                }
                3 => world.continent.continent_scale = 1000,
                4 => {
                    world.continent.continent_scale = 777;
                    rivers.lakes.chance = 0.5;
                    world.seed = 7;
                }
                _ => {}
            }
            (world, rivers)
        }

        /// `new FancyContinentGenerator(new Seed(continent_seed), context)`, whose `FancyContinent` builds its
        /// `FancyRiverGenerator` from the same `GeneratorContext` (seed root `world.seed`).
        fn continent(
            variant: i32,
            continent_seed: i32,
            registry: &Arc<TerrainRegistry>,
        ) -> (FancyContinentGenerator, i32) {
            let (world, rivers) = settings(variant);
            let levels = Levels::new(world.properties.world_height, world.properties.sea_level);
            let continent = FancyContinentGenerator::new(
                &mut Seed::new(continent_seed),
                world.seed as i32,
                world,
                rivers,
                levels,
                Arc::clone(registry),
            );
            (continent, world.continent.continent_scale)
        }

        #[test]
        fn generates_networks_like_engine() {
            let registry = Arc::new(TerrainRegistry::new());
            let ids = [0x1234_5678, -1, 0x7fff_ffff_0000_0005];
            let mut fold = Fold(17);
            let (mut applies, mut networks, mut nodes) = (0, 0, 0);
            for variant in 0..5 {
                for continent_seed in [12_345, -98_765] {
                    let (continent, scale) = continent(variant, continent_seed, &registry);
                    let generator = continent.river_generator();
                    for (i, id) in (0..).zip(ids) {
                        let (x, z) = (200 * i - 300, 150 - 100 * i);
                        let map = generator.generate_rivers(x, z, id);
                        fold.mix(map.x());
                        fold.mix(map.z());
                        fold.mix(map.networks.len() as i32);
                        networks += map.networks.len();
                        for network in &map.networks {
                            nodes += mix_network(&mut fold, network);
                        }
                        let step = 0.15 * scale as f32;
                        for gx in 0..=40 {
                            for gz in 0..=40 {
                                let px = -3.0 * scale as f32 + step * gx as f32;
                                let pz = -3.0 * scale as f32 + step * gz as f32;
                                for fill in [None, Some(0), Some(1)] {
                                    let mut cell = fill_from(&FILLS, fill, &registry);
                                    map.apply(&mut cell, px, pz);
                                    fold.cell(&cell, &registry);
                                    applies += 1;
                                }
                            }
                        }
                    }
                }
            }
            assert_eq!(
                (applies, networks, nodes, fold.0),
                (151_290, 1163, 1702, -1_536_805_052)
            );
        }

        /// `FancyContinentGenerator.getRivermap`: the river cache over the continent's own `FancyRiverGenerator`, with
        /// a continent seed that differs from the context's, folded like `generates_networks_like_engine`.
        #[test]
        fn rivermaps_match_engine() {
            const CELLS: [(i32, i32); 6] = [
                (-7, 9),
                (0, 0),
                (17, -9),
                (i32::MIN, i32::MAX),
                (-1, -1),
                (3, i32::MIN),
            ];
            let registry = Arc::new(TerrainRegistry::new());
            let mut fold = Fold(17);
            let (mut networks, mut nodes) = (0, 0);
            for variant in 0..5 {
                let (continent, _) = continent(variant, -98_765, &registry);
                for (x, z) in CELLS {
                    let map = continent.rivermap(x, z);
                    assert!(Arc::ptr_eq(&map, &continent.rivermap(x, z)));
                    fold.mix(map.x());
                    fold.mix(map.z());
                    fold.mix(map.networks.len() as i32);
                    networks += map.networks.len();
                    for network in &map.networks {
                        nodes += mix_network(&mut fold, network);
                    }
                }
            }
            assert_eq!((networks, nodes, fold.0), (1216, 1781, 92_682_963));
        }

        #[test]
        fn samples_match_engine() {
            let registry = Arc::new(TerrainRegistry::new());
            let (continent, _) = continent(0, 12_345, &registry);
            let generator = continent.river_generator();
            let map = generator.generate_rivers(17, -9, 0x1234_5678);
            assert_eq!(map.networks.len(), 38);
            let roots: Vec<_> = map
                .networks
                .iter()
                .take(6)
                .map(|network| {
                    let river = network.river_carver.river;
                    (
                        [river.x1, river.z1, river.x2, river.z2].map(f32::to_bits),
                        network.children.len(),
                    )
                })
                .collect();
            #[rustfmt::skip]
            assert_eq!(
                roots,
                [
                    ([0xc3e5_f4e5, 0x4191_a4cb, 0xc456_588e, 0x4506_637e], 1),
                    ([0xc4c1_f108, 0xc312_7834, 0xc4b0_53fb, 0xc560_a93d], 1),
                    ([0x28a9_e4fb, 0x431a_0540, 0x2a6f_9582, 0x44d9_32dc], 1),
                    ([0xc503_2035, 0xc4f3_bfb9, 0xc585_3b03, 0xc4f3_bfb9], 1),
                    ([0xc5b0_9a1a, 0xc49d_7a96, 0xc5b3_a681, 0xc3ad_2cd5], 1),
                    ([0xc5b3_fdbd, 0xc4a0_15ba, 0xc5ad_d99f, 0xc50c_25fd], 1),
                ]
            );
            let applied: Vec<_> = (0..12)
                .map(|i| {
                    let river = map.networks[i / 2].river_carver.river;
                    let t = if i % 2 == 0 { 0.3 } else { 0.7 };
                    let x = river.x1 + (river.x2 - river.x1) * t;
                    let z = river.z1 + (river.z2 - river.z1) * t;
                    let mut cell = Cell::new(&registry);
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
                })
                .collect();
            #[rustfmt::skip]
            assert_eq!(
                applied,
                [
                    (0x3e83_4515, 0x39fa_0000, false, 0), (0x3e87_874b, 0x39f2_4800, false, 0),
                    (0x3e83_3949, 0x3634_0000, false, 0), (0x3e87_a7c1, 0x3a9c_ea00, false, 0),
                    (0x3e83_5fae, 0x3a58_4c00, false, 0), (0x3e66_3602, 0x3a30_6400, true, 5),
                    (0x3e83_43f4, 0x39b8_5000, false, 0), (0x3e88_01d2, 0x3b42_0500, false, 0),
                    (0x3e83_8b92, 0x3b23_2000, false, 0), (0x3e87_a222, 0x3946_1000, false, 0),
                    (0x3e83_3f35, 0x39d4_f800, false, 0), (0x3e87_8c95, 0x39a9_e000, false, 0),
                ]
            );
        }
    }
}
