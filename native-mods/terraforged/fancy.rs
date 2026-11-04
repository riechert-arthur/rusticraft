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

use std::sync::Arc;

use horizon_core::random::{LegacyRandom, RandomSource};

use super::{
    terraforged::{Domain, Line, SourceKind, cos, java_max, java_min, lerp, map, noise_curve, sin},
    terraforged_continent::noise_warp,
    terraforged_engine::{
        Cell, ControlPoints, Levels, RiverSettings, Seed, TerrainRegistry, WorldSettings,
    },
    terraforged_populator::Populator,
    terraforged_river::{
        BaseRiverGenerator, GenWarp, River, RiverCarver, RiverCarverSettings, RiverNetwork,
        RiverNetworkBuilder, RiverWarp, Rivermap, Variance,
    },
    terraforged_util::{
        pack, pack_floats, pack_mix_int_float, unpack_left, unpack_left_float, unpack_right_float,
    },
};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec2f {
    pub x: f32,
    pub y: f32,
}

impl Vec2f {
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

#[derive(Clone, Debug)]
pub struct Segment {
    pub a: Vec2f,
    pub b: Vec2f,
    pub dx: f32,
    pub dy: f32,
    pub length: f32,
    pub length2: f32,
    pub scale_a: f32,
    pub scale2_a: f32,
    pub scale_b: f32,
    pub scale2_b: f32,
}

impl Segment {
    pub fn new(a: Vec2f, b: Vec2f, scale_a: f32, scale_b: f32) -> Self {
        let dx = b.x - a.x;
        let dy = b.y - a.y;
        let length2 = dx * dx + dy * dy;
        Self {
            a,
            b,
            dx,
            dy,
            length: f64::from(length2).sqrt() as f32,
            length2,
            scale_a,
            scale2_a: scale_a * scale_a,
            scale_b,
            scale2_b: scale_b * scale_b,
        }
    }

    pub fn min_x(&self) -> f32 {
        java_min(self.a.x, self.b.x)
    }

    pub fn min_y(&self) -> f32 {
        java_min(self.a.y, self.b.y)
    }

    pub fn max_x(&self) -> f32 {
        java_max(self.a.x, self.b.x)
    }

    pub fn max_y(&self) -> f32 {
        java_max(self.a.y, self.b.y)
    }

    pub fn max_scale(&self) -> f32 {
        java_max(self.scale_a, self.scale_b)
    }

    pub fn translate(&self, offset: Vec2f) -> Self {
        Self::new(
            Vec2f::new(self.a.x + offset.x, self.a.y + offset.y),
            Vec2f::new(self.b.x + offset.x, self.b.y + offset.y),
            self.scale_a,
            self.scale_b,
        )
    }
}

pub struct Island {
    id: i32,
    segments: Vec<Segment>,
    _control_points: ControlPoints,
    coast2: f32,
    deep_ocean2: f32,
    shallow_ocean: f32,
    shallow_ocean2: f32,
    _coast: f32,
    _inland: f32,
    radius: f32,
    deep_mod: f32,
    shallow_mod: f32,
    center: Vec2f,
    min: Vec2f,
    max: Vec2f,
}

impl Island {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: i32,
        segments: Vec<Segment>,
        control_points: ControlPoints,
        deep_ocean: f32,
        shallow_ocean: f32,
        coast: f32,
        inland: f32,
    ) -> Self {
        let mut x = 0.0;
        let mut y = 0.0;
        let points = segments.len() + 1;
        let mut min_x = f32::MAX;
        let mut min_z = f32::MAX;
        // Java's `Float.MIN_VALUE`: the smallest positive subnormal, not `f32::MIN_POSITIVE`.
        let mut max_x = f32::from_bits(1);
        let mut max_z = f32::from_bits(1);
        let mut max_radius = coast;
        for (index, segment) in segments.iter().enumerate() {
            min_x = java_min(min_x, segment.min_x());
            min_z = java_min(min_z, segment.min_y());
            max_x = java_max(max_x, segment.max_x());
            max_z = java_max(max_z, segment.max_y());
            max_radius = java_max(max_radius, segment.max_scale() * coast);
            if index == 0 {
                x += segment.a.x;
                y += segment.a.y;
            }
            x += segment.b.x;
            y += segment.b.y;
        }
        max_x += coast;
        min_x -= coast;
        max_z += coast;
        min_z -= coast;
        let max_dimension = java_max(max_x - min_x, max_z - min_z);
        let deep_mod = 0.25;
        Self {
            id,
            segments,
            _control_points: control_points,
            coast2: coast * coast,
            deep_ocean2: deep_ocean * deep_ocean,
            shallow_ocean,
            shallow_ocean2: shallow_ocean * shallow_ocean,
            _coast: coast,
            _inland: inland,
            radius: max_dimension,
            deep_mod,
            shallow_mod: 1.0 - deep_mod,
            center: Vec2f::new(x / points as f32, y / points as f32),
            min: Vec2f::new(min_x - max_radius, min_z - max_radius),
            max: Vec2f::new(max_x + max_radius, max_z + max_radius),
        }
    }

    pub const fn id(&self) -> i32 {
        self.id
    }

    pub fn segments(&self) -> &[Segment] {
        &self.segments
    }

    pub const fn radius(&self) -> f32 {
        self.radius
    }

    pub const fn coast(&self) -> f32 {
        self.shallow_ocean
    }

    pub fn translate(&mut self, offset: Vec2f) {
        self.center = Vec2f::new(self.center.x + offset.x, self.center.y + offset.y);
        self.min = Vec2f::new(self.min.x + offset.x, self.min.y + offset.y);
        self.max = Vec2f::new(self.max.x + offset.x, self.max.y + offset.y);
        for segment in &mut self.segments {
            *segment = segment.translate(offset);
        }
    }

    pub const fn min(&self) -> Vec2f {
        self.min
    }

    pub const fn max(&self) -> Vec2f {
        self.max
    }

    pub const fn center(&self) -> Vec2f {
        self.center
    }

    pub fn overlaps(&self, other: &Self) -> bool {
        self.overlaps_bounds(other.min, other.max)
    }

    pub const fn overlaps_bounds(&self, min: Vec2f, max: Vec2f) -> bool {
        self.min.x < max.x && self.max.x > min.x && self.min.y < max.y && self.max.y > min.y
    }

    pub const fn contains(&self, x: f32, z: f32) -> bool {
        x > self.min.x && x < self.max.x && z > self.min.y && z < self.max.y
    }

    pub fn edge_value(&self, x: f32, y: f32) -> f32 {
        let value = self.edge_distance2(x, y, self.deep_ocean2);
        self.process(
            java_min(self.deep_ocean2, value),
            java_min(self.shallow_ocean2, value),
        )
    }

    pub fn land_value(&self, x: f32, y: f32) -> f32 {
        let mut value = self.edge_distance2(x, y, self.shallow_ocean2);
        if value < self.shallow_ocean2 {
            value = (self.shallow_ocean2 - value) / self.shallow_ocean2;
            noise_curve(value, 0.75, 4.0)
        } else {
            0.0
        }
    }

    fn edge_distance2(&self, x: f32, y: f32, min_distance2: f32) -> f32 {
        let mut value = min_distance2;
        for segment in &self.segments {
            let mut projection = (x - segment.a.x) * segment.dx + (y - segment.a.y) * segment.dy;
            projection /= segment.length2;
            let (point_x, point_y, scale) = if projection < 0.0 {
                (segment.a.x, segment.a.y, segment.scale2_a)
            } else if projection > 1.0 {
                (segment.b.x, segment.b.y, segment.scale2_b)
            } else {
                (
                    segment.a.x + projection * segment.dx,
                    segment.a.y + projection * segment.dy,
                    lerp(segment.scale2_a, segment.scale2_b, projection),
                )
            };
            value = java_min(Line::dist2(x, y, point_x, point_y) / scale, value);
        }
        value
    }

    fn process(&self, mut deep_value: f32, mut shallow_value: f32) -> f32 {
        if deep_value == self.deep_ocean2 {
            return 0.0;
        }
        if deep_value > self.shallow_ocean2 {
            deep_value =
                (deep_value - self.shallow_ocean2) / (self.deep_ocean2 - self.shallow_ocean2);
            deep_value = 1.0 - deep_value;
            deep_value *= deep_value;
            return deep_value * self.deep_mod;
        }
        if shallow_value == self.shallow_ocean2 {
            return self.deep_mod;
        }
        if shallow_value > self.coast2 {
            shallow_value = (shallow_value - self.coast2) / (self.shallow_ocean2 - self.coast2);
            shallow_value = 1.0 - shallow_value;
            return self.deep_mod + shallow_value * self.shallow_mod;
        }
        1.0
    }
}

/// Engine's `FancyContinent`: a main island and four arms of three ever smaller satellite islands, built from a
/// `java.util.Random` seeded with the generator's first seed. Engine's `FancyContinent` also builds the continent's
/// `FancyRiverGenerator` and answers `generateRivers` with it; here `FancyRiverGenerator` owns the continent generator.
pub struct FancyContinent {
    islands: Vec<Island>,
}

impl FancyContinent {
    pub fn new(seed: i32, nodes: i32, radius: f32, control_points: ControlPoints) -> Self {
        // `new Random(seed)`: the int seed widens to a long with sign extension.
        let mut random = LegacyRandom::new(i64::from(seed));
        Self {
            islands: Self::generate_islands(control_points, 3, nodes, radius, &mut random),
        }
    }

    /// `getValue`: the largest island edge value (`Math.max(v, value)` keeps a NaN).
    pub fn value(&self, x: f32, y: f32) -> f32 {
        self.islands.iter().fold(0.0, |value, island| {
            java_max(island.edge_value(x, y), value)
        })
    }

    pub fn main(&self) -> &Island {
        &self.islands[0]
    }

    pub fn islands(&self) -> &[Island] {
        &self.islands
    }

    /// `getMin`: the smallest island minimum, packed as raw float bits.
    pub fn min(&self) -> i64 {
        let mut x = f32::MAX;
        let mut z = f32::MAX;
        for island in &self.islands {
            x = java_min(x, island.min().x);
            z = java_min(z, island.min().y);
        }
        pack_floats(x, z)
    }

    /// `getMax`: as in Engine, the largest island *minimum* (not maximum), starting at `Float.MIN_VALUE` (the
    /// smallest positive subnormal).
    pub fn max(&self) -> i64 {
        let mut x = f32::from_bits(1);
        let mut z = f32::from_bits(1);
        for island in &self.islands {
            x = java_max(x, island.min().x);
            z = java_max(z, island.min().y);
        }
        pack_floats(x, z)
    }

    /// `getLandValue`: the largest island land value.
    pub fn land_value(&self, x: f32, y: f32) -> f32 {
        self.islands.iter().fold(0.0, |value, island| {
            java_max(island.land_value(x, y), value)
        })
    }

    /// `getValueId`: the id of the first island with the largest edge value (-1 when none is above 0) in the high
    /// half, that value's raw bits in the low half.
    pub fn value_id(&self, x: f32, y: f32) -> i64 {
        let mut id = -1;
        let mut value = 0.0_f32;
        for island in &self.islands {
            let next = island.edge_value(x, y);
            if next > value {
                value = next;
                id = island.id();
            }
            value = java_max(next, value);
        }
        pack_mix_int_float(id, value)
    }

    /// `generateIslands`: the main island (id 0), then per direction one yaw draw and `island_count` islands
    /// (ids 1, 2, … in build order), each placed `radius()` away from the previous one in its arm.
    fn generate_islands(
        control_points: ControlPoints,
        island_count: i32,
        node_count: i32,
        radius: f32,
        random: &mut LegacyRandom,
    ) -> Vec<Island> {
        const DIRECTIONS: i32 = 4;
        let main = Self::generate(0, control_points, node_count, radius, random);
        let mut islands = Vec::with_capacity((1 + island_count * DIRECTIONS) as usize);
        islands.push(main);
        let yaw_step = 1.0 / DIRECTIONS as f32 * java_pi2();
        for direction in 0..DIRECTIONS {
            let mut previous = 0;
            let mut nodes = 2.max(node_count - 1);
            let mut next_radius = radius * 0.5;
            let yaw = yaw_step * direction as f32 + random.next_float() * yaw_step;
            for _ in 0..island_count {
                let id = islands.len() as i32;
                let mut next = Self::generate(id, control_points, nodes, next_radius, random);
                let angle = yaw + next_float(random, -0.2, 0.2);
                let distance = islands[previous].radius();
                let dx = sin(angle * java_pi2()) * distance;
                let dz = cos(angle * java_pi2()) * distance;
                let center = islands[previous].center();
                next.translate(Vec2f::new(center.x + dx, center.y + dz));
                nodes = 2.max(nodes - 1);
                next_radius *= 0.8;
                islands.push(next);
                previous = islands.len() - 1;
            }
        }
        islands
    }

    /// `generate`: a polyline of `nodes - 1` segments from the origin, each draw in Engine's order (start scale,
    /// start yaw, then per segment length, yaw turn and end scale).
    fn generate(
        id: i32,
        control_points: ControlPoints,
        nodes: i32,
        radius: f32,
        random: &mut LegacyRandom,
    ) -> Island {
        let min_scale = 0.75;
        let max_scale = 2.5;
        let min_length = radius * 1.5;
        let max_length = radius * 3.5;
        // `1.5707964f`.
        let max_yaw = f32::from_bits(0x3fc9_0fdb);
        let segment_count = (nodes - 1).max(0) as usize;
        let mut segments = Vec::with_capacity(segment_count);
        let mut point_a = Vec2f::new(0.0, 0.0);
        let mut scale_a = next_float(random, min_scale, max_scale);
        let mut previous_yaw = next_float(random, 0.0, java_pi2());
        for _ in 0..segment_count {
            let length = next_float(random, min_length, max_length);
            let yaw = previous_yaw + next_float(random, -max_yaw, max_yaw);
            let dx = sin(yaw) * length;
            let dz = cos(yaw) * length;
            let point_b = Vec2f::new(point_a.x + dx, point_a.y + dz);
            let scale_b = next_float(random, min_scale, max_scale);
            segments.push(Segment::new(point_a, point_b, scale_a, scale_b));
            previous_yaw = yaw;
            point_a = point_b;
            scale_a = scale_b;
        }
        Island::new(
            id,
            segments,
            control_points,
            radius * 3.0,
            radius * 1.25,
            radius,
            radius * 0.975,
        )
    }
}

/// Engine's `FancyContinentGenerator` (`ContinentType.EXPERIMENTAL`): one [`FancyContinent`] at the origin,
/// sampled through three summed domain warps. Its river map is not ported.
pub struct FancyContinentGenerator {
    frequency: f32,
    warp: Domain,
    source: FancyContinent,
}

impl FancyContinentGenerator {
    /// Draws four seeds: the continent's, then the first warp's (Simplex, its y source at `seed + 1`) and the
    /// two Perlin warps' (`Domain.warp(seed, …)` defaults to Perlin).
    pub fn new(seed: &mut Seed, settings: WorldSettings) -> Self {
        let warp_scale = settings.continent.continent_scale / 2;
        let warp_strength = (f64::from(warp_scale) * 0.4) as f32;
        let source = FancyContinent::new(
            seed.next(),
            4,
            0.2,
            ControlPoints::new(settings.control_points),
        );
        let frequency = 1.0 / settings.continent.continent_scale as f32;
        let warp = noise_warp(
            SourceKind::Simplex,
            seed.next(),
            warp_scale,
            2,
            warp_strength,
        )
        .add(noise_warp(SourceKind::Perlin, seed.next(), 80, 2, 40.0))
        .add(noise_warp(SourceKind::Perlin, seed.next(), 20, 1, 15.0));
        Self {
            frequency,
            warp,
            source,
        }
    }

    pub const fn source(&self) -> &FancyContinent {
        &self.source
    }

    /// `getEdgeValue`: the continent's value at the warped point times the frequency.
    pub fn edge_value(&self, x: f32, y: f32) -> f32 {
        let point_x = self.warp.x(x, y) * self.frequency;
        let point_y = self.warp.y(x, y) * self.frequency;
        self.source.value(point_x, point_y)
    }

    /// `getLandValue`: the continent's land value at the warped point, mapped from 0.2..0.4.
    pub fn land_value(&self, x: f32, y: f32) -> f32 {
        let point_x = self.warp.x(x, y) * self.frequency;
        let point_y = self.warp.y(x, y) * self.frequency;
        map(self.source.land_value(point_x, point_y), 0.2, 0.4, 0.2)
    }

    /// `getNearestCenter`: ignores its arguments; half of `getMax - getMin` in blocks, truncated like `(int)`.
    pub fn nearest_center(&self, _x: f32, _z: f32) -> i64 {
        let min = self.source.min();
        let max = self.source.max();
        let width = unpack_left_float(max) - unpack_left_float(min);
        let height = unpack_right_float(max) - unpack_right_float(min);
        pack(
            (width * 0.5 / self.frequency) as i32,
            (height * 0.5 / self.frequency) as i32,
        )
    }
}

/// Engine's `FancyRiverGenerator`: per island of the fancy continent, main rivers projected across its segments
/// and out of its two endpoints until the continent value falls below 0.1, each forked and given wetlands as soon
/// as it is placed. In Engine, `FancyContinent` builds one in its constructor and `FancyContinentGenerator`'s
/// river cache asks it for maps; here it owns the continent generator and the caller passes what Engine reads
/// from the `GeneratorContext` (see `BaseRiverGenerator::new`).
pub struct FancyRiverGenerator {
    base: BaseRiverGenerator<FancyContinentGenerator>,
    frequency: f32,
}

impl FancyRiverGenerator {
    /// `END_VALUE`: the continent value below which a river ends.
    const END_VALUE: f32 = 0.1;
    const MAIN_PADDING: Variance = Variance::of(0.05, 0.1);
    const MAIN_JITTER: Variance = Variance::of(-0.2, 0.4);

    pub fn new(
        continent: FancyContinentGenerator,
        root_seed: i32,
        world: &WorldSettings,
        rivers: &RiverSettings,
        levels: Levels,
        registry: Arc<TerrainRegistry>,
    ) -> Self {
        Self {
            frequency: 1.0 / world.continent.continent_scale as f32,
            base: BaseRiverGenerator::new(continent, root_seed, world, rivers, levels, registry),
        }
    }

    /// `generateRivers(x, z, id)`: no shuffle and no second fork pass, unlike the base class; every island's
    /// roots become networks in placement order.
    pub fn generate_rivers(&self, x: i32, z: i32, id: i64) -> Rivermap {
        let mut random = LegacyRandom::new(id.wrapping_add(i64::from(self.base.seed)));
        let warp = GenWarp::new(id as i32, self.base.continent_scale);
        let mut networks = Vec::with_capacity(32);
        let mut roots = Vec::with_capacity(16);
        for island in self.base.continent.source().islands() {
            self.generate_roots(island, &mut random, &warp, &mut roots);
            networks.extend(roots.drain(..).map(RiverNetworkBuilder::build));
        }
        Rivermap::new(x, z, networks, warp)
    }

    fn generate_roots(
        &self,
        island: &Island,
        random: &mut LegacyRandom,
        warp: &GenWarp,
        roots: &mut Vec<RiverNetworkBuilder>,
    ) {
        let segments = island.segments();
        let line_count = 1.max(8 - island.id());
        let end_count = 4.max(12 - island.id());
        for (index, segment) in segments.iter().enumerate() {
            let end = index == 0 || index == segments.len() - 1;
            let river_count = if end { line_count - 1 } else { line_count };
            self.collect_segment_roots(island, segment, river_count, random, warp, roots);
        }
        // Engine reads `segments[0]` unchecked; every island has at least one segment (`nodes >= 2`).
        let first = &segments[0];
        self.collect_point_roots(
            island,
            first.a,
            first.scale_a,
            end_count,
            random,
            warp,
            roots,
        );
        let last = &segments[segments.len() - 1];
        self.collect_point_roots(island, last.b, last.scale_b, end_count, random, warp, roots);
    }

    fn collect_segment_roots(
        &self,
        island: &Island,
        segment: &Segment,
        count: i32,
        random: &mut LegacyRandom,
        warp: &GenWarp,
        roots: &mut Vec<RiverNetworkBuilder>,
    ) {
        let dx = segment.dx;
        let dy = segment.dy;
        let normal_x = dy / segment.length;
        let normal_z = -dx / segment.length;
        let step_size = 1.0 / (count + 2) as f32;
        for index in 0..count {
            let progress = step_size + step_size * index as f32;
            if progress > 1.0 {
                return;
            }
            let start_x = segment.a.x + dx * progress;
            let start_z = segment.a.y + dy * progress;
            let radius_scale = lerp(segment.scale_a, segment.scale_b, progress);
            let radius = island.coast() * radius_scale;
            let direction: f32 = if random.next_boolean() { -1.0 } else { 1.0 };
            let direction_x = normal_x * direction + Self::MAIN_JITTER.next(random);
            let direction_z = normal_z * direction + Self::MAIN_JITTER.next(random);
            let scale = self.extend_scale(
                island.id(),
                start_x,
                start_z,
                direction_x,
                direction_z,
                radius,
            );
            if scale == 0.0 {
                continue;
            }
            let start_padding = Self::MAIN_PADDING.next(random);
            let x1 = start_x + direction * direction_x * radius * start_padding;
            let z1 = start_z + direction * direction_z * radius * start_padding;
            let x2 = start_x + direction_x * radius * scale;
            let z2 = start_z + direction_z * radius * scale;
            self.add_root(x1, z1, x2, z2, random, warp, roots);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn collect_point_roots(
        &self,
        island: &Island,
        point: Vec2f,
        radius_scale: f32,
        count: i32,
        random: &mut LegacyRandom,
        warp: &GenWarp,
        roots: &mut Vec<RiverNetworkBuilder>,
    ) {
        let yaw_step = java_pi2() / count as f32;
        let radius = island.coast() * radius_scale;
        for index in 0..count {
            let yaw = yaw_step * index as f32;
            let dx = cos(yaw);
            let dz = sin(yaw);
            let scale = self.extend_scale(island.id(), point.x, point.y, dx, dz, radius);
            if scale == 0.0 {
                continue;
            }
            let start_padding = Self::MAIN_PADDING.next(random);
            let start_x = point.x + dx * start_padding * radius;
            let start_z = point.y + dz * start_padding * radius;
            let end_x = point.x + dx * radius * scale;
            let end_z = point.y + dz * radius * scale;
            if self.base.continent.source().value(end_x, end_z) > Self::END_VALUE {
                continue;
            }
            self.add_root(start_x, start_z, end_x, end_z, random, warp, roots);
        }
    }

    /// `addRoot(x1, z1, x2, z2, main, random, warp, roots)`: the root is added to `roots` before its forks are
    /// generated, so they are tested against it (by Engine's parent rule) as well as the island's other roots.
    #[allow(clippy::too_many_arguments)]
    fn add_root(
        &self,
        x1: f32,
        z1: f32,
        x2: f32,
        z2: f32,
        random: &mut LegacyRandom,
        warp: &GenWarp,
        roots: &mut Vec<RiverNetworkBuilder>,
    ) {
        let config = self.base.main;
        let river = River::new(
            x1 / self.frequency,
            z1 / self.frequency,
            x2 / self.frequency,
            z2 / self.frequency,
        );
        if BaseRiverGenerator::<FancyContinentGenerator>::river_overlaps(river, None, roots) {
            return;
        }
        // `creatSettings` draws the valley curve first, then the valley size, then the warp.
        let valley_curve = RiverCarver::valley_type(random);
        let valley_size = River::VALLEY_WIDTH as f32 * River::FORK_VALLEY.next(random);
        let settings = RiverCarverSettings {
            connecting: false,
            fade_in: config.fade,
            valley_size,
            valley_curve,
        };
        let river_warp = RiverWarp::create_range(0.1, 0.85, random);
        let carver = RiverCarver::new(
            river,
            river_warp,
            config,
            settings,
            self.base.levels,
            &self.base.registry,
        );
        roots.push(RiverNetwork::builder(carver));
        let index = roots.len() - 1;
        self.base.generate_forks(
            roots,
            &[index],
            River::FORK_SPACING,
            self.base.fork,
            random,
            warp,
            0,
        );
        self.base.generate_wetlands(&mut roots[index], random);
    }

    /// `getExtendScale`: steps out from `start` by `0.075 * radius` up to 25 times; 0 when a step leaves the
    /// island (`getValueId`'s id differs) or never reaches a value below 0.1.
    #[allow(clippy::too_many_arguments)]
    fn extend_scale(
        &self,
        island_id: i32,
        start_x: f32,
        start_z: f32,
        dx: f32,
        dz: f32,
        radius: f32,
    ) -> f32 {
        let mut scale = 1.0_f32;
        for _ in 0..25 {
            let x = start_x + dx * radius * scale;
            let z = start_z + dz * radius * scale;
            let packed = self.base.continent.source().value_id(x, z);
            if unpack_left(packed) != island_id {
                return 0.0;
            }
            if unpack_right_float(packed) < Self::END_VALUE {
                return scale;
            }
            scale += 0.075;
        }
        0.0
    }
}

impl Populator for FancyContinentGenerator {
    /// Writes `continentX`, `continentZ` and `continentId` (all 0) and `continentEdge`. Engine does not override
    /// `getValue`, so [`Populator::value`] keeps the default: the scratch cell's untouched `value` (always 0).
    fn apply(&self, cell: &mut Cell, x: f32, y: f32) {
        cell.continent_x = 0;
        cell.continent_z = 0;
        cell.continent_id = 0.0;
        cell.continent_edge = self.edge_value(x, y);
    }
}

/// `(float) Math.PI * 2`.
fn java_pi2() -> f32 {
    f32::from_bits(0x40c9_0fdb)
}

/// `FancyContinent.nextFloat`: `min + nextFloat() * (max - min)`.
fn next_float(random: &mut LegacyRandom, min: f32, max: f32) -> f32 {
    min + random.next_float() * (max - min)
}

#[cfg(test)]
mod tests {
    use super::super::terraforged_continent::tests::{XS as BLOCK_XS, filled, mix_cell};
    use super::super::terraforged_engine::{TerrainRegistry, WorldControlPoints};
    use super::*;

    fn vector_bits(vector: Vec2f) -> [u32; 2] {
        [vector.x.to_bits(), vector.y.to_bits()]
    }

    #[test]
    fn fancy_island_matches_engine_0_3_0() {
        let segments = vec![
            Segment::new(Vec2f::new(0.0, 0.0), Vec2f::new(120.0, -40.0), 0.75, 1.25),
            Segment::new(Vec2f::new(120.0, -40.0), Vec2f::new(200.0, 90.0), 1.25, 2.5),
            Segment::new(Vec2f::new(200.0, 90.0), Vec2f::new(-50.0, 180.0), 2.5, 0.9),
        ];
        let mut island = Island::new(
            7,
            segments,
            ControlPoints::new(WorldControlPoints::default()),
            300.0,
            125.0,
            100.0,
            97.5,
        );
        assert_eq!(island.id(), 7);
        assert_eq!(island.radius().to_bits(), 0x43e10000);
        assert_eq!(island.coast().to_bits(), 0x42fa0000);
        assert_eq!(vector_bits(island.center()), [0x42870000, 0x42660000]);
        assert_eq!(vector_bits(island.min()), [0xc3c80000, 0xc3c30000]);
        assert_eq!(vector_bits(island.max()), [0x44098000, 0x44048000]);
        for ((x, y), edge, land, contains) in [
            ((0.0, 0.0), 0x3f800000, 0x3f3b3fe2, true),
            ((100.0, 0.0), 0x3f800000, 0x3f32cb05, true),
            ((250.0, 200.0), 0x3f800000, 0x3f1967bd, true),
            ((-100.0, -100.0), 0x3e14e339, 0, true),
            ((1000.0, 1000.0), 0, 0, false),
        ] {
            assert_eq!(island.edge_value(x, y).to_bits(), edge);
            assert_eq!(island.land_value(x, y).to_bits(), land);
            assert_eq!(island.contains(x, y), contains);
        }

        let other = Island::new(
            8,
            vec![Segment::new(
                Vec2f::new(500.0, 500.0),
                Vec2f::new(600.0, 500.0),
                1.0,
                1.0,
            )],
            ControlPoints::default(),
            300.0,
            125.0,
            100.0,
            97.5,
        );
        assert!(island.overlaps(&other));
        island.translate(Vec2f::new(50.0, -25.0));
        assert_eq!(vector_bits(island.center()), [0x42eb0000, 0x42020000]);
        assert_eq!(vector_bits(island.min()), [0xc3af0000, 0xc3cf8000]);
        assert_eq!(vector_bits(island.max()), [0x44160000, 0x43fc8000]);
        assert!(island.overlaps(&other));
        let segment = &island.segments()[0];
        assert_eq!(
            [
                segment.dx.to_bits(),
                segment.dy.to_bits(),
                segment.length.to_bits(),
                segment.length2.to_bits(),
                segment.max_scale().to_bits(),
            ],
            [0x42f00000, 0xc2200000, 0x42fcfb72, 0x467a0000, 0x3fa00000]
        );
    }

    const NAN: f32 = f32::NAN;
    const INF: f32 = f32::INFINITY;

    /// Per island: the polyline's points (x, y, scale), as in the oracle's `POINTS`.
    const POINTS: [&[[f32; 3]]; 7] = [
        &[
            [0.0, 0.0, 0.75],
            [120.0, -40.0, 1.25],
            [200.0, 90.0, 2.5],
            [-50.0, 180.0, 0.9],
        ],
        &[[-10.0, -20.0, 1.0], [-30.0, -5.0, 0.5], [-70.0, -90.0, 2.0]],
        &[[-0.0, -0.0, 1.0], [0.0, 0.0, 1.0], [-0.0, 25.5, 1.5]],
        &[],
        &[
            [5.0, 5.0, 1.0],
            [5.0, 5.0, 2.0],
            [40.0, -12.5, 0.0],
            [40.0, NAN, 1.0],
        ],
        &[[1e30, -1e30, 1.0], [INF, 3e38, 1.0], [-3e38, -INF, NAN]],
        &[
            [-1500.5, 2400.75, 1.7],
            [-1312.25, 2611.125, 0.35],
            [-1001.0, 2580.5, 1.15],
            [-777.75, 2201.25, 0.6],
            [-650.125, 1999.5, 1.9],
        ],
    ];
    /// Per island: deep ocean, shallow ocean, coast and inland.
    const RADII: [[f32; 4]; 7] = [
        [300.0, 125.0, 100.0, 97.5],
        [50.0, 20.0, 0.0, 0.0],
        [0.0, 0.0, 0.0, 0.0],
        [30.0, 12.5, 10.0, 9.75],
        [90.0, 37.5, 30.0, 29.25],
        [3e38, 1e38, 1e30, 9e29],
        [660.0, 275.0, 220.0, 214.5],
    ];
    const XS: [f32; 16] = [
        0.0,
        -0.0,
        5.0,
        12.25,
        -98.75,
        100.0,
        250.0,
        -1312.25,
        -1000.0,
        2500.0,
        1e9,
        -3e38,
        INF,
        -INF,
        NAN,
        f32::from_bits(1),
    ];
    const OFFSETS: [[f32; 2]; 4] = [[50.0, -25.0], [-0.0, -0.0], [NAN, 1.0], [1e38, -1e38]];

    fn mix(h: &mut i64, value: i64) {
        *h = h.wrapping_mul(1_000_003) ^ value;
    }

    fn mix_f(h: &mut i64, value: f32) {
        let bits = if value.is_nan() {
            0x7fc0_0000
        } else {
            value.to_bits() as i32
        };
        mix(h, i64::from(bits));
    }

    fn island(index: usize) -> Island {
        let points = POINTS[index];
        let segments = points
            .windows(2)
            .map(|pair| {
                Segment::new(
                    Vec2f::new(pair[0][0], pair[0][1]),
                    Vec2f::new(pair[1][0], pair[1][1]),
                    pair[0][2],
                    pair[1][2],
                )
            })
            .collect();
        let [deep_ocean, shallow_ocean, coast, inland] = RADII[index];
        Island::new(
            index as i32 * 7 - 3,
            segments,
            ControlPoints::new(WorldControlPoints::default()),
            deep_ocean,
            shallow_ocean,
            coast,
            inland,
        )
    }

    /// Every field the oracles print for a segment, in their order.
    fn segment_fields(s: &Segment) -> [f32; 17] {
        [
            s.a.x,
            s.a.y,
            s.b.x,
            s.b.y,
            s.dx,
            s.dy,
            s.length,
            s.length2,
            s.scale_a,
            s.scale2_a,
            s.scale_b,
            s.scale2_b,
            s.min_x(),
            s.min_y(),
            s.max_x(),
            s.max_y(),
            s.max_scale(),
        ]
    }

    /// Folds what the oracle prints for one island into `h`.
    fn fold(h: &mut i64, island: &Island, all: &[Island]) {
        mix(h, i64::from(island.id()));
        mix_f(h, island.radius());
        mix_f(h, island.coast());
        for vector in [island.center(), island.min(), island.max()] {
            mix_f(h, vector.x);
            mix_f(h, vector.y);
        }
        for s in island.segments() {
            for value in segment_fields(s) {
                mix_f(h, value);
            }
        }
        for x in XS {
            for z in XS {
                mix_f(h, island.edge_value(x, z));
                mix_f(h, island.land_value(x, z));
                mix(h, i64::from(island.contains(x, z)));
            }
        }
        for other in all {
            mix(h, i64::from(island.overlaps(other)));
        }
    }

    #[test]
    fn fancy_islands_match_engine_0_3_0_over_a_grid() {
        // `island N hash=` and `translated N hash=` from the oracle's output.
        const ISLANDS: [i64; 7] = [
            4_471_371_439_962_242_810,
            -2_980_711_422_084_806_754,
            5_819_305_975_358_599_233,
            5_436_465_002_603_715_890,
            2_661_829_515_760_187_032,
            -3_173_365_622_338_592_339,
            -2_177_918_726_559_654_814,
        ];
        const TRANSLATED: [i64; 7] = [
            8_439_386_740_593_808_674,
            8_212_017_515_462_460_511,
            -8_916_857_709_666_907_142,
            -2_941_846_210_725_880_895,
            2_537_679_223_634_290_945,
            -4_324_800_807_680_692_811,
            -1_581_689_032_412_099_271,
        ];
        let all: Vec<Island> = (0..POINTS.len()).map(island).collect();
        let mut h = 17;
        for (index, expected) in ISLANDS.into_iter().enumerate() {
            fold(&mut h, &all[index], &all);
            assert_eq!(h, expected, "island {index}");
        }
        for (index, expected) in TRANSLATED.into_iter().enumerate() {
            for [x, y] in OFFSETS {
                let mut island = island(index);
                island.translate(Vec2f::new(x, y));
                fold(&mut h, &island, &all);
            }
            assert_eq!(h, expected, "translated {index}");
        }
    }

    /// The configs of the Java oracle (`.local/terraforged/oracles/continent/FancyContinentOracle.java`):
    /// (seed, settings).
    fn config(index: usize) -> (i32, WorldSettings) {
        let mut settings = WorldSettings {
            seed: 246_813_579,
            ..WorldSettings::default()
        };
        let continent = &mut settings.continent;
        let seed = match index {
            1 => {
                continent.continent_scale = 777;
                i32::MAX - 3
            }
            2 => {
                continent.continent_scale = 1;
                settings.control_points = WorldControlPoints {
                    inland: 0.9,
                    shallow_ocean: 0.05,
                    ..WorldControlPoints::default()
                };
                -98_765
            }
            3 => {
                continent.continent_scale = i32::MAX;
                i32::MIN + 1
            }
            4 => {
                continent.continent_scale = 2;
                7
            }
            5 => {
                continent.continent_scale = 0;
                0
            }
            6 => {
                continent.continent_scale = -3001;
                -1
            }
            _ => 12_345,
        };
        (seed, settings)
    }

    /// Continent units: -2.5..=2.5 in steps of 0.125, then NaN, +inf, -0 and 1e30 (the oracle's `US`).
    fn unit_grid() -> Vec<f32> {
        let mut values: Vec<f32> = (0..41).map(|i| -2.5 + 0.125 * i as f32).collect();
        values.extend([NAN, INF, -0.0, 1e30]);
        values
    }

    /// Folds what the oracle prints for one config into `h`; returns the seed cursor after construction.
    fn fold_continent(h: &mut i64, index: usize) -> i32 {
        let registry = TerrainRegistry::new();
        let (seed_value, settings) = config(index);
        let mut seed = Seed::new(seed_value);
        let generator = FancyContinentGenerator::new(&mut seed, settings);
        mix(h, i64::from(seed.get()));
        let source = generator.source();
        mix(h, source.islands().len() as i64);
        for island in source.islands() {
            mix(h, i64::from(island.id()));
            mix_f(h, island.radius());
            mix_f(h, island.coast());
            for vector in [island.center(), island.min(), island.max()] {
                mix_f(h, vector.x);
                mix_f(h, vector.y);
            }
            mix(h, island.segments().len() as i64);
            for s in island.segments() {
                for value in segment_fields(s) {
                    mix_f(h, value);
                }
            }
        }
        mix(h, source.min());
        mix(h, source.max());
        let units = unit_grid();
        for &x in &units {
            for &z in &units {
                mix_f(h, source.value(x, z));
                mix_f(h, source.land_value(x, z));
                mix(h, source.value_id(x, z));
            }
        }
        for x in BLOCK_XS {
            for z in BLOCK_XS {
                let mut cell = Cell::new(&registry);
                generator.apply(&mut cell, x, z);
                mix_cell(h, &cell);
                let mut cell = filled(&registry);
                generator.apply(&mut cell, x, z);
                mix_cell(h, &cell);
                mix_f(h, generator.value(&registry, x, z));
                mix_f(h, generator.edge_value(x, z));
                mix_f(h, generator.land_value(x, z));
                mix(h, generator.nearest_center(x, z));
            }
        }
        seed.get()
    }

    #[test]
    fn fancy_continents_match_engine_0_3_0_over_a_grid() {
        // Per config: the seed cursor after construction and the running hash after it (`config N hash=`).
        const EXPECTED: [(i32, i64); 7] = [
            (12_349, 9_139_903_238_874_810_953),
            (i32::MIN, 6_311_856_010_399_455_219),
            (-98_761, -4_215_715_806_185_393_804),
            (-2_147_483_643, 4_614_737_165_176_145_525),
            (11, -8_057_806_375_809_892_168),
            (4, 7_600_711_732_524_442_215),
            (3, -1_961_742_243_151_308_495),
        ];
        let mut h = 17;
        for (index, (seed, hash)) in EXPECTED.into_iter().enumerate() {
            assert_eq!(fold_continent(&mut h, index), seed, "config {index} seed");
            assert_eq!(h, hash, "config {index} hash");
        }
    }

    #[test]
    fn fancy_continent_matches_engine_0_3_0() {
        let registry = TerrainRegistry::new();
        let mut seed = Seed::new(12_345);
        let generator = FancyContinentGenerator::new(&mut seed, WorldSettings::default());
        assert_eq!(seed.get(), 12_349);
        let source = generator.source();
        let ids: Vec<i32> = source.islands().iter().map(Island::id).collect();
        assert_eq!(ids, (0..13).collect::<Vec<_>>());
        // The main island, from the oracle's first `island` lines.
        let main = source.main();
        assert_eq!(main.radius().to_bits(), 0x3fa640ad);
        assert_eq!(vector_bits(main.center()), [0xbf0efba6, 0xbe8c43f0]);
        assert_eq!(vector_bits(main.min()), [0xbfc52cd5, 0xbfa55276]);
        assert_eq!(main.segments().len(), 3);
        assert_eq!(source.min() as u64, 0xc0311076bff40052);
        assert_eq!(source.max() as u64, 0x3f17b54d3e668bf2);
        assert_eq!(
            generator.nearest_center(0.0, 0.0) as u64,
            0x000013ae00000c7d
        );
        // (x, z, edge = land): inside the main island, then far out at sea.
        for (x, z, edge) in [
            (0.0, 0.0, 0x3f800000),
            (12.25, -98.75, 0x3f800000),
            (-4096.5, 8192.25, 0),
        ] {
            assert_eq!(generator.edge_value(x, z).to_bits(), edge);
            assert_eq!(generator.land_value(x, z).to_bits(), edge);
            assert_eq!(generator.value(&registry, x, z).to_bits(), 0);
            let mut cell = filled(&registry);
            generator.apply(&mut cell, x, z);
            assert_eq!(cell.continent_edge.to_bits(), edge);
            assert_eq!(
                (
                    cell.continent_x,
                    cell.continent_z,
                    cell.continent_id.to_bits()
                ),
                (0, 0, 0)
            );
            assert_eq!(cell.value.to_bits(), 0.75_f32.to_bits());
        }
    }
}
