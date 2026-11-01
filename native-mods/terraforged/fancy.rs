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
        Domain, Interpolation, Line, Module, Source, SourceConfig, SourceKind, cos, java_max,
        java_min, lerp, map, noise_curve, sin,
    },
    terraforged_engine::{Cell, ControlPoints, Seed, WorldSettings},
    terraforged_populator::Populator,
    terraforged_util::{
        pack, pack_floats, pack_mix_int_float, unpack_left_float, unpack_right_float,
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
        let yaw_step = 1.0 / directions as f32 * java_pi2();
        for direction in 0..directions {
            let mut previous_index = 0;
            let mut nodes = 2.max(node_count - 1);
            let mut next_radius = radius * 0.5;
            let yaw = yaw_step * direction as f32 + random.next_float() * yaw_step;
            for island in 0..island_count {
                let mut next =
                    Self::generate(island as i32, control_points, nodes, next_radius, random);
                let angle = yaw + next_float(random, -0.2, 0.2);
                let previous = &islands[previous_index];
                let distance = previous.radius();
                let angle = angle * java_pi2();
                let offset_x = previous.center().x + sin(angle) * distance;
                let offset_y = previous.center().y + cos(angle) * distance;
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
            let point_b = Vec2f::new(point_a.x + sin(yaw) * length, point_a.y + cos(yaw) * length);
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
        map(self.source.land_value(point_x, point_y), 0.2, 0.4, 0.2)
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
            for value in [
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
            ] {
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
            let mut cell = Cell::new(&super::super::terraforged_engine::TerrainRegistry::new());
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
}
