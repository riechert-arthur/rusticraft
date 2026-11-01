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

use horizon_core::random::{LegacyRandom, RandomSource};

use super::{
    terraforged::{
        Domain, Interpolation, Module, Source, SourceConfig, SourceKind, cos_lookup, noise_curve,
        noise_map, sin_lookup,
    },
    terraforged_engine::{Cell, ControlPoints, Levels, RiverSettings, Seed, WorldSettings},
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
        self.a.x.min(self.b.x)
    }

    pub fn min_y(&self) -> f32 {
        self.a.y.min(self.b.y)
    }

    pub fn max_x(&self) -> f32 {
        self.a.x.max(self.b.x)
    }

    pub fn max_y(&self) -> f32 {
        self.a.y.max(self.b.y)
    }

    pub fn max_scale(&self) -> f32 {
        self.scale_a.max(self.scale_b)
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
        let mut max_x = f32::MIN_POSITIVE;
        let mut max_z = f32::MIN_POSITIVE;
        let mut max_radius = coast;
        for (index, segment) in segments.iter().enumerate() {
            min_x = min_x.min(segment.min_x());
            min_z = min_z.min(segment.min_y());
            max_x = max_x.max(segment.max_x());
            max_z = max_z.max(segment.max_y());
            max_radius = max_radius.max(segment.max_scale() * coast);
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
        let max_dimension = (max_x - min_x).max(max_z - min_z);
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
        self.process(self.deep_ocean2.min(value), self.shallow_ocean2.min(value))
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
                    segment.scale2_a + projection * (segment.scale2_b - segment.scale2_a),
                )
            };
            let dx = point_x - x;
            let dy = point_y - y;
            value = value.min((dx * dx + dy * dy) / scale);
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

pub struct FancyContinent {
    islands: Vec<Island>,
}

impl FancyContinent {
    pub fn new(seed: i32, nodes: usize, radius: f32, control_points: ControlPoints) -> Self {
        let mut random = LegacyRandom::new(i64::from(seed));
        Self {
            islands: Self::generate_islands(control_points, 3, nodes, radius, &mut random),
        }
    }

    pub fn value(&self, x: f32, y: f32) -> f32 {
        self.islands
            .iter()
            .fold(0.0_f32, |value, island| value.max(island.edge_value(x, y)))
    }

    pub fn main(&self) -> &Island {
        &self.islands[0]
    }

    pub fn islands(&self) -> &[Island] {
        &self.islands
    }

    pub fn min(&self) -> i64 {
        let mut x = f32::MAX;
        let mut z = f32::MAX;
        for island in &self.islands {
            x = x.min(island.min().x);
            z = z.min(island.min().y);
        }
        pack_floats(x, z)
    }

    pub fn max(&self) -> i64 {
        let mut x = f32::MIN_POSITIVE;
        let mut z = f32::MIN_POSITIVE;
        for island in &self.islands {
            x = x.max(island.min().x);
            z = z.max(island.min().y);
        }
        pack_floats(x, z)
    }

    pub fn land_value(&self, x: f32, y: f32) -> f32 {
        self.islands
            .iter()
            .fold(0.0_f32, |value, island| value.max(island.land_value(x, y)))
    }

    pub fn value_id(&self, x: f32, y: f32) -> i64 {
        let mut id = -1;
        let mut value = 0.0_f32;
        for island in &self.islands {
            let next = island.edge_value(x, y);
            if next > value {
                value = next;
                id = island.id();
            }
            value = value.max(next);
        }
        pack_mix_int_float(id, value)
    }

    fn generate_islands(
        control_points: ControlPoints,
        island_count: usize,
        node_count: usize,
        radius: f32,
        random: &mut LegacyRandom,
    ) -> Vec<Island> {
        let directions = 4;
        let main = Self::generate(0, control_points, node_count, radius, random);
        let mut islands = Vec::with_capacity(1 + island_count * directions);
        islands.push(main);
        let mut next_id = 1;
        let yaw_step = 1.0 / directions as f32 * java_pi2();
        for direction in 0..directions {
            let mut previous_index = 0;
            let mut nodes = 2.max(node_count - 1);
            let mut next_radius = radius * 0.5;
            let yaw = yaw_step * direction as f32 + random.next_float() * yaw_step;
            for _ in 0..island_count {
                let mut next = Self::generate(next_id, control_points, nodes, next_radius, random);
                next_id += 1;
                let angle = yaw + next_float(random, -0.2, 0.2);
                let previous = &islands[previous_index];
                let distance = previous.radius();
                let angle = angle * java_pi2();
                let offset_x = previous.center().x + sin_lookup(angle) * distance;
                let offset_y = previous.center().y + cos_lookup(angle) * distance;
                next.translate(Vec2f::new(offset_x, offset_y));
                nodes = 2.max(nodes - 1);
                next_radius *= 0.8;
                islands.push(next);
                previous_index = islands.len() - 1;
            }
        }
        islands
    }

    fn generate(
        id: i32,
        control_points: ControlPoints,
        nodes: usize,
        radius: f32,
        random: &mut LegacyRandom,
    ) -> Island {
        let min_scale = 0.75;
        let max_scale = 2.5;
        let min_length = radius * 1.5;
        let max_length = radius * 3.5;
        let max_yaw = f32::from_bits(0x3fc9_0fdb);
        let mut segments = Vec::with_capacity(nodes - 1);
        let mut point_a = Vec2f::new(0.0, 0.0);
        let mut scale_a = next_float(random, min_scale, max_scale);
        let mut previous_yaw = next_float(random, 0.0, java_pi2());
        for _ in 0..nodes - 1 {
            let length = next_float(random, min_length, max_length);
            let yaw = previous_yaw + next_float(random, -max_yaw, max_yaw);
            let point_b = Vec2f::new(
                point_a.x + sin_lookup(yaw) * length,
                point_a.y + cos_lookup(yaw) * length,
            );
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

pub struct FancyContinentGenerator {
    frequency: f32,
    warp: Domain,
    source: FancyContinent,
}

impl FancyContinentGenerator {
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
        .add(noise_warp(SourceKind::Simplex, seed.next(), 80, 2, 40.0))
        .add(noise_warp(SourceKind::Simplex, seed.next(), 20, 1, 15.0));
        Self {
            frequency,
            warp,
            source,
        }
    }

    pub const fn source(&self) -> &FancyContinent {
        &self.source
    }

    pub fn edge_value(&self, x: f32, y: f32) -> f32 {
        let point_x = self.warp.x(x, y) * self.frequency;
        let point_y = self.warp.y(x, y) * self.frequency;
        self.source.value(point_x, point_y)
    }

    pub fn land_value(&self, x: f32, y: f32) -> f32 {
        let point_x = self.warp.x(x, y) * self.frequency;
        let point_y = self.warp.y(x, y) * self.frequency;
        noise_map(self.source.land_value(point_x, point_y), 0.2, 0.4, 0.2)
    }

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

pub struct FancyRiverGenerator {
    base: BaseRiverGenerator<FancyContinentGenerator>,
    frequency: f32,
}

impl FancyRiverGenerator {
    const MAIN_PADDING: Variance = Variance::of(0.05, 0.1);
    const MAIN_JITTER: Variance = Variance::of(-0.2, 0.4);

    pub fn new(
        continent: FancyContinentGenerator,
        root_seed: i32,
        world: WorldSettings,
        rivers: RiverSettings,
        levels: Levels,
    ) -> Self {
        Self {
            frequency: 1.0 / world.continent.continent_scale as f32,
            base: BaseRiverGenerator::new(continent, root_seed, world, rivers, levels),
        }
    }

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
            self.collect_segment_roots(
                island,
                segment,
                if end { line_count - 1 } else { line_count },
                random,
                warp,
                roots,
            );
        }
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
        let normal_x = segment.dy / segment.length;
        let normal_z = -segment.dx / segment.length;
        let step_size = 1.0 / (count + 2) as f32;
        for index in 0..count {
            let progress = step_size + step_size * index as f32;
            if progress > 1.0 {
                return;
            }
            let start_x = segment.a.x + segment.dx * progress;
            let start_z = segment.a.y + segment.dy * progress;
            let radius_scale = segment.scale_a + (segment.scale_b - segment.scale_a) * progress;
            let radius = island.coast() * radius_scale;
            let direction = if random.next_boolean() { -1.0 } else { 1.0 };
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
            self.add_root(
                start_x + direction * direction_x * radius * start_padding,
                start_z + direction * direction_z * radius * start_padding,
                start_x + direction_x * radius * scale,
                start_z + direction_z * radius * scale,
                random,
                warp,
                roots,
            );
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
            let direction_x = cos_lookup(yaw);
            let direction_z = sin_lookup(yaw);
            let scale = self.extend_scale(
                island.id(),
                point.x,
                point.y,
                direction_x,
                direction_z,
                radius,
            );
            if scale == 0.0 {
                continue;
            }
            let start_padding = Self::MAIN_PADDING.next(random);
            let end_x = point.x + direction_x * radius * scale;
            let end_z = point.y + direction_z * radius * scale;
            if self.base.continent.source().value(end_x, end_z) > 0.1 {
                continue;
            }
            self.add_root(
                point.x + direction_x * start_padding * radius,
                point.y + direction_z * start_padding * radius,
                end_x,
                end_z,
                random,
                warp,
                roots,
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn extend_scale(
        &self,
        island_id: i32,
        start_x: f32,
        start_z: f32,
        direction_x: f32,
        direction_z: f32,
        radius: f32,
    ) -> f32 {
        let mut scale = 1.0;
        for _ in 0..25 {
            let packed = self.base.continent.source().value_id(
                start_x + direction_x * radius * scale,
                start_z + direction_z * radius * scale,
            );
            if unpack_left(packed) != island_id {
                return 0.0;
            }
            if unpack_right_float(packed) < 0.1 {
                return scale;
            }
            scale += 0.075;
        }
        0.0
    }

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
        let river = River::new(
            x1 / self.frequency,
            z1 / self.frequency,
            x2 / self.frequency,
            z2 / self.frequency,
        );
        if roots.iter().any(|root| root.overlaps(river, None, 250.0)) {
            return;
        }
        let settings = RiverCarverSettings {
            valley_curve: RiverCarver::valley_type(random),
            fade_in: self.base.main.fade,
            valley_size: 275.0 * River::FORK_VALLEY.next(random),
            connecting: false,
        };
        let river_warp = RiverWarp::create_range(0.1, 0.85, random);
        let carver = RiverCarver::new(
            river,
            river_warp,
            self.base.main,
            settings,
            self.base.levels,
        );
        let mut network = RiverNetwork::builder(carver);
        let mut occupied = Vec::new();
        for root in roots.iter() {
            collect_rivers(root, &mut occupied);
        }
        occupied.push(river);
        self.base.generate_forks(
            &mut network,
            River::FORK_SPACING,
            self.base.fork,
            random,
            warp,
            &mut occupied,
            0,
        );
        self.base.generate_wetlands(&mut network, random);
        roots.push(network);
    }
}

fn collect_rivers(network: &RiverNetworkBuilder, rivers: &mut Vec<River>) {
    rivers.push(network.carver.river);
    for child in &network.children {
        collect_rivers(child, rivers);
    }
}

impl Populator for FancyContinentGenerator {
    fn apply(&self, cell: &mut Cell, x: f32, y: f32) {
        cell.continent_x = 0;
        cell.continent_z = 0;
        cell.continent_id = 0.0;
        cell.continent_edge = self.edge_value(x, y);
    }
}

fn java_pi2() -> f32 {
    f32::from_bits(0x40c9_0fdb)
}

fn next_float(random: &mut LegacyRandom, min: f32, max: f32) -> f32 {
    min + random.next_float() * (max - min)
}

fn noise_warp(kind: SourceKind, seed: i32, scale: i32, octaves: usize, strength: f32) -> Domain {
    let source = |seed| {
        Module::source(Source::new(
            kind,
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
    use super::super::terraforged_engine::WorldControlPoints;
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

    #[test]
    fn fancy_continent_matches_engine_0_3_0() {
        let settings = WorldSettings::default();
        let mut seed = Seed::new(12_345);
        let generator = FancyContinentGenerator::new(&mut seed, settings);
        assert_eq!(seed.get(), 12_349);
        assert_eq!(generator.source().islands().len(), 13);
        assert_eq!(generator.source().min() as u64, 0xc0311076bff40052);
        assert_eq!(generator.source().max() as u64, 0x3f17b54d3e668bf2);
        assert_eq!(
            generator.nearest_center(0.0, 0.0) as u64,
            0x000013ae00000c7d
        );

        for (x, y, source, edge, land, value_id) in [
            (
                0.0,
                0.0,
                0x3f800000,
                0x3f800000,
                0x3f800000,
                0x000000003f800000,
            ),
            (
                12.25,
                -98.75,
                0x3f800000,
                0x3f800000,
                0x3f800000,
                0x000000003f800000,
            ),
            (-4096.5, 8192.25, 0, 0, 0, 0xffffffff00000000),
            (123456.75, -0.125, 0, 0, 0, 0xffffffff00000000),
            (50000.0, 50000.0, 0, 0, 0, 0xffffffff00000000),
        ] {
            assert_eq!(
                generator.source().value(x / 3000.0, y / 3000.0).to_bits(),
                source
            );
            assert_eq!(generator.edge_value(x, y).to_bits(), edge);
            assert_eq!(generator.land_value(x, y).to_bits(), land);
            assert_eq!(
                generator.source().value_id(x / 3000.0, y / 3000.0) as u64,
                value_id
            );
            let mut cell = Cell::default();
            generator.apply(&mut cell, x, y);
            assert_eq!(cell.continent_edge.to_bits(), edge);
            assert_eq!(
                (
                    cell.continent_x,
                    cell.continent_z,
                    cell.continent_id.to_bits()
                ),
                (0, 0, 0)
            );
        }
    }

    #[test]
    fn fancy_rivers_match_engine_0_3_0() {
        let mut world = WorldSettings::default();
        world.seed = 12_345;
        let mut seed = Seed::new(12_345);
        let continent = FancyContinentGenerator::new(&mut seed, world);
        let levels = Levels::from_properties(world.properties);
        let generator =
            FancyRiverGenerator::new(continent, 12_345, world, RiverSettings::default(), levels);
        let map = generator.generate_rivers(17, -9, 0x1234_5678);
        assert_eq!((map.x(), map.z(), map.network_count()), (17, -9, 38));
        assert_eq!(
            map.root_rivers()
                .into_iter()
                .take(6)
                .map(|(river, children)| {
                    (
                        river.x1.to_bits(),
                        river.z1.to_bits(),
                        river.x2.to_bits(),
                        river.z2.to_bits(),
                        children,
                    )
                })
                .collect::<Vec<_>>(),
            vec![
                (0xc3e5f4e5, 0x4191a4cb, 0xc456588e, 0x4506637e, 1),
                (0xc4c1f108, 0xc3127834, 0xc4b053fb, 0xc560a93d, 1),
                (0x28a9e4fb, 0x431a0540, 0x2a6f9582, 0x44d932dc, 1),
                (0xc5032035, 0xc4f3bfb9, 0xc5853b03, 0xc4f3bfb9, 1),
                (0xc5b09a1a, 0xc49d7a96, 0xc5b3a681, 0xc3ad2cd5, 1),
                (0xc5b3fdbd, 0xc4a015ba, 0xc5add99f, 0xc50c25fd, 1),
            ]
        );

        for (x, z, value, mask, erosion, terrain) in [
            (0xc424a980, 0x448786c8, 0x3e855e88, 0x378a0000, false, 0),
            (0xc4b92282, 0xc4e9d0c0, 0x3e6aac83, 0x37f00000, true, 5),
            (0x2a026911, 0x446c7384, 0x3e812da9, 0x35a00000, true, 0),
            (0xc546cb1e, 0xc4f3bfb9, 0x3e855679, 0x36580000, false, 0),
            (0xc5b2204e, 0xc448c5cb, 0x3e85ab68, 0x3aee1a00, false, 0),
            (0xc5b0ebae, 0xc4dc30da, 0x3e7b1891, 0x00000000, true, 7),
            (0xc5cf509f, 0xc4cc4db0, 0x3e856321, 0x39f8b000, false, 0),
            (0xc5d76c5a, 0xc4b69567, 0x3e730e94, 0x3b834580, true, 5),
            (0xc5d990da, 0xc4afd5fa, 0x3e856c55, 0x3b688300, false, 0),
            (0xc5dbeb49, 0xc4a96799, 0x3e854ba9, 0x398aa000, false, 0),
            (0xc5cfb068, 0xc438cac7, 0x3e859ea8, 0x3aa32000, false, 0),
            (0xc5a68d00, 0xc4dd776c, 0x3e856a38, 0x37c90000, false, 0),
        ] {
            let mut cell = Cell::default();
            cell.value = 0.6;
            cell.river_mask = 1.0;
            cell.continent_edge = 0.4;
            map.apply(&mut cell, f32::from_bits(x), f32::from_bits(z));
            assert_eq!(cell.value.to_bits(), value);
            assert_eq!(cell.river_mask.to_bits(), mask);
            assert_eq!(cell.erosion_mask, erosion);
            assert_eq!(cell.terrain.id(), terrain);
        }
    }
}
