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
    terraforged::{Interpolation, clamp, map},
    terraforged_engine::{Terrain, TerrainRegistry},
};

/// TerraForged's `worldgen.noise.continent.river.RiverNode` (the mod's continent rivers, not Engine's
/// `rivermap`): a river or lake segment from `a` to `b` with its height (`h`) and radius (`r`) at each end and
/// a sideways displacement that bows the middle of the segment. No `PartialEq`: the Java record's `equals`
/// compares floats with `Float.compare` (NaN equals NaN, -0 differs from 0), unlike Rust's `==`.
#[derive(Clone, Copy, Debug)]
pub struct RiverNode {
    pub ax: f32,
    pub ay: f32,
    pub bx: f32,
    pub by: f32,
    pub ah: f32,
    pub bh: f32,
    pub ar: f32,
    pub br: f32,
    pub displacement: f32,
}

impl RiverNode {
    /// `getProjection`: where `(x, y)` projects onto the segment, clamped to 0..1 (NaN, e.g. for a zero-length
    /// segment, passes through).
    pub fn projection(self, x: f32, y: f32) -> f32 {
        let delta_x = self.bx - self.ax;
        let delta_y = self.by - self.ay;
        let value = ((x - self.ax) * delta_x + (y - self.ay) * delta_y)
            / (delta_x * delta_x + delta_y * delta_y);
        clamp(value, 0.0, 1.0)
    }

    /// `getDistance2`: the squared distance from `(x, y)` to the displaced point at `projection`.
    pub fn distance_squared(self, x: f32, y: f32, projection: f32) -> f32 {
        let pad = 0.05_f32;
        let alpha = map(projection, pad, 1.0 - pad, 1.0 - pad * 2.0);
        let alpha = if alpha < 0.5 {
            alpha / 0.5
        } else {
            (1.0 - alpha) / 0.5
        };
        let alpha = Interpolation::Curve3.apply(alpha) * self.displacement;
        let point_x = self.x(projection) - (self.by - self.ay) * alpha;
        let point_y = self.y(projection) + (self.bx - self.ax) * alpha;
        // `Line.dist2(x, y, px, py)`: the deltas run from (x, y) to the point.
        let delta_x = point_x - x;
        let delta_y = point_y - y;
        delta_x * delta_x + delta_y * delta_y
    }

    /// `getDistance(x, y, t)`: `NoiseUtil.sqrt` (`(float) Math.sqrt`) of [`Self::distance_squared`].
    pub fn distance(self, x: f32, y: f32, projection: f32) -> f32 {
        (self.distance_squared(x, y, projection) as f64).sqrt() as f32
    }

    /// `getDistance(t, d2)`: 1 at the centre line falling to 0 at the radius at `projection`; 0 at or beyond it.
    pub fn radial_distance(self, projection: f32, distance_squared: f32) -> f32 {
        let radius = self.radius(projection);
        if distance_squared >= radius * radius {
            0.0
        } else {
            1.0 - (distance_squared as f64).sqrt() as f32 / radius
        }
    }

    pub fn x(self, projection: f32) -> f32 {
        self.ax + projection * (self.bx - self.ax)
    }
    pub fn y(self, projection: f32) -> f32 {
        self.ay + projection * (self.by - self.ay)
    }
    pub fn height(self, projection: f32) -> f32 {
        self.ah + projection * (self.bh - self.ah)
    }
    pub fn radius(self, projection: f32) -> f32 {
        self.ar + projection * (self.br - self.ar)
    }
}

/// TerraForged's `NodeSample`: the nearest node found for one terrain (`type` in Java). `distance` is NaN
/// until [`Self::reset`] (invalid) and `f32::MAX` after it.
#[derive(Clone, Debug)]
pub struct NodeSample {
    pub projection: f32,
    pub distance: f32,
    pub position: f32,
    pub level: f32,
    pub terrain: Terrain,
}

impl NodeSample {
    pub fn new(terrain: Terrain) -> Self {
        Self {
            projection: 0.0,
            distance: f32::NAN,
            position: 0.0,
            level: 0.0,
            terrain,
        }
    }

    pub fn is_invalid(&self) -> bool {
        self.distance.is_nan()
    }

    pub fn reset(&mut self) -> &mut Self {
        self.distance = f32::MAX;
        self.projection = 0.0;
        self.position = 0.0;
        self.level = 0.0;
        self
    }

    pub fn invalidate(&mut self) -> &mut Self {
        self.distance = f32::NAN;
        self
    }
}

/// TerraForged's `CarverSample`: the river and lake samples of one position.
pub struct CarverSample {
    pub river: NodeSample,
    pub lake: NodeSample,
}

impl CarverSample {
    /// `new CarverSample()`: `terrains` is the world's registry, whose ids 5 and 6 are Engine's
    /// `TerrainType.RIVER` and `TerrainType.LAKE`.
    pub fn new(terrains: &TerrainRegistry) -> Self {
        Self {
            river: NodeSample::new(terrains.get_by_id(5)),
            lake: NodeSample::new(terrains.get_by_id(6)),
        }
    }

    pub fn reset(&mut self) -> &mut Self {
        self.river.reset();
        self.lake.reset();
        self
    }
}

/// TerraForged's `RiverPieces`: the river and lake nodes of one cell, in insertion order. The Java arrays grow
/// one slot at a time and `reset` only zeroes the counts; a `Vec` gives the same contents. `RiverPieces.NONE`
/// is `RiverPieces::default()`.
#[derive(Default)]
pub struct RiverPieces {
    pub rivers: Vec<RiverNode>,
    pub lakes: Vec<RiverNode>,
}

impl RiverPieces {
    pub fn reset(&mut self) -> &mut Self {
        self.rivers.clear();
        self.lakes.clear();
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node() -> RiverNode {
        RiverNode {
            ax: -1.25,
            ay: 2.5,
            bx: 4.75,
            by: -0.5,
            ah: 0.9,
            bh: 0.2,
            ar: 0.125,
            br: 0.75,
            displacement: -0.2,
        }
    }

    #[test]
    fn river_node_geometry_matches_java_raw_bits() {
        let expected = [
            [0, 0x40aa0000, 0x40138341, 0, 0x3f666666, 0x3e000000],
            [
                0x3e888889, 0x3cb600ac, 0x3e18a1a2, 0x3efa58a2, 0x3f369d03, 0x3e955556,
            ],
            [
                0x3f19999a, 0x3e1b830c, 0x3ec786ed, 0x3e61e44c, 0x3ef5c28e, 0x3f000000,
            ],
            [
                0x3f800000, 0x3ea00000, 0x3f0f1bbd, 0x3e8260b2, 0x3e4ccccc, 0x3f400000,
            ],
        ];
        for (index, (x, y)) in [(-3.0, 4.0), (0.0, 1.0), (2.0, 0.0), (5.0, -1.0)]
            .into_iter()
            .enumerate()
        {
            let node = node();
            let projection = node.projection(x, y);
            let distance = node.distance_squared(x, y, projection);
            assert_eq!(
                [
                    projection.to_bits(),
                    distance.to_bits(),
                    node.distance(x, y, projection).to_bits(),
                    node.radial_distance(projection, distance).to_bits(),
                    node.height(projection).to_bits(),
                    node.radius(projection).to_bits(),
                ],
                expected[index]
            );
        }
        let mut degenerate = node();
        degenerate.bx = degenerate.ax;
        degenerate.by = degenerate.ay;
        assert!(degenerate.projection(0.0, 0.0).is_nan());
    }

    const VALUES: [f32; 20] = [
        0.0,
        -0.0,
        1.0,
        -1.0,
        0.5,
        0.05,
        0.95,
        0.25,
        2.5,
        -3.75,
        1e-3,
        1e10,
        -1e10,
        3e38,
        -3e38,
        f32::MAX,
        f32::from_bits(1),
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NAN,
    ];

    const NODES: [[f32; 9]; 4] = [
        [-1.25, 2.5, 4.75, -0.5, 0.9, 0.2, 0.125, 0.75, -0.2],
        [10.0, 10.0, 90.0, 30.0, 0.4, 0.1, 6.0, 2.0, 0.35],
        [-1000.5, 300.25, -1000.5, 900.75, 0.0, 1.0, 40.0, 40.0, 1.0],
        [3.0, 3.0, 3.0, 3.0, 0.5, 0.5, 1.0, 2.0, 0.5],
    ];

    struct Fnv(u64, usize);

    impl Fnv {
        fn new() -> Self {
            Self(0xcbf2_9ce4_8422_2325, 0)
        }

        /// Canonical NaN, as the oracle's `floatToIntBits`: NaN payload signs are not portable.
        fn add(&mut self, value: f32) {
            let bits = if value.is_nan() {
                0x7fc0_0000
            } else {
                value.to_bits()
            };
            for byte in bits.to_le_bytes() {
                self.0 = (self.0 ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
            }
            self.1 += 1;
        }

        fn point(&mut self, node: RiverNode, x: f32, y: f32) {
            let projection = node.projection(x, y);
            let distance = node.distance_squared(x, y, projection);
            self.add(projection);
            self.add(distance);
            self.add(node.distance(x, y, projection));
            self.add(node.radial_distance(projection, distance));
            self.add(node.x(projection));
            self.add(node.y(projection));
            self.add(node.height(projection));
            self.add(node.radius(projection));
        }

        fn check(&mut self, name: &str, count: usize, hash: u64) {
            assert_eq!((self.1, self.0), (count, hash), "{name}");
            *self = Self::new();
        }
    }

    fn from_fields(f: [f32; 9]) -> RiverNode {
        RiverNode {
            ax: f[0],
            ay: f[1],
            bx: f[2],
            by: f[3],
            ah: f[4],
            bh: f[5],
            ar: f[6],
            br: f[7],
            displacement: f[8],
        }
    }

    /// `RiverNodesOracle` (`.local/terraforged/oracles/river_nodes/`, the jar's RiverNode bytecode): every
    /// method over NaN, infinities, signed zeros, subnormal and huge inputs, as points, parameters and nodes.
    #[test]
    fn river_node_edge_inputs_match_the_jar() {
        let mut fnv = Fnv::new();
        for fields in NODES {
            let node = from_fields(fields);
            for x in VALUES {
                for y in VALUES {
                    fnv.point(node, x, y);
                }
            }
        }
        fnv.check("points", 12_800, 0x3737_f68f_01bd_f5d6);
        for fields in NODES {
            let node = from_fields(fields);
            for t in VALUES {
                fnv.add(node.distance_squared(1.5, -2.0, t));
                fnv.add(node.distance(-7.0, 0.25, t));
                fnv.add(node.x(t));
                fnv.add(node.y(t));
                fnv.add(node.height(t));
                fnv.add(node.radius(t));
                for distance in VALUES {
                    fnv.add(node.radial_distance(t, distance));
                }
            }
        }
        fnv.check("params", 2_080, 0x64cd_f159_41da_fdc4);
        let points = [
            (0.0, 0.0),
            (2.0, 1.0),
            (-3.0, 4.0),
            (50.0, 20.0),
            (-1000.5, 600.0),
        ];
        for fields in NODES {
            for field in 0..9 {
                for value in VALUES {
                    let mut fields = fields;
                    fields[field] = value;
                    let node = from_fields(fields);
                    for (x, y) in points {
                        fnv.point(node, x, y);
                    }
                    fnv.add(node.distance_squared(4.0, -2.5, 0.3));
                }
            }
        }
        fnv.check("edges", 29_520, 0x99a8_b4c8_791d_2dcb);
    }

    /// The oracle's `terrain`/`new`/`invalidate`/`reset`/`pieces` lines.
    #[test]
    fn samples_and_pieces_match_the_jar() {
        let terrains = TerrainRegistry::default();
        let mut samples = CarverSample::new(&terrains);
        assert!(
            samples
                .river
                .terrain
                .is_same(&terrains.get_by_name("river").unwrap())
        );
        assert!(
            samples
                .lake
                .terrain
                .is_same(&terrains.get_by_name("lake").unwrap())
        );
        assert_eq!(samples.river.terrain.id(), 5);
        assert_eq!(samples.lake.terrain.id(), 6);
        let fields = |sample: &NodeSample| {
            [
                sample.projection,
                sample.distance,
                sample.position,
                sample.level,
            ]
            .map(f32::to_bits)
        };
        assert!(samples.river.is_invalid());
        assert_eq!(fields(&samples.river), [0, 0x7fc0_0000, 0, 0]);
        samples.river.projection = 0.5;
        samples.river.position = 2.0;
        samples.river.level = 0.75;
        assert!(samples.river.invalidate().is_invalid());
        assert_eq!(
            fields(&samples.river),
            [0x3f00_0000, 0x7fc0_0000, 0x4000_0000, 0x3f40_0000]
        );
        samples.reset();
        assert!(!samples.river.is_invalid());
        assert_eq!(fields(&samples.river), [0, 0x7f7f_ffff, 0, 0]);
        assert!(!samples.lake.is_invalid());
        assert_eq!(samples.lake.distance.to_bits(), 0x7f7f_ffff);

        let node = |ax: f32| from_fields([ax, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
        let mut pieces = RiverPieces::default();
        pieces.rivers.extend((0..11).map(|i| node(i as f32)));
        pieces.lakes.extend((0..3).map(|i| node(-i as f32)));
        let count = |nodes: &[RiverNode]| nodes.len() as i32;
        let mut seen = vec![count(&pieces.rivers), count(&pieces.lakes)];
        seen.extend(
            pieces
                .rivers
                .iter()
                .chain(&pieces.lakes)
                .map(|n| n.ax as i32),
        );
        pieces.reset();
        seen.extend([count(&pieces.rivers), count(&pieces.lakes)]);
        pieces.rivers.push(node(42.0));
        seen.extend([count(&pieces.rivers), pieces.rivers[0].ax as i32]);
        let none = RiverPieces::default();
        seen.extend([count(&none.rivers), count(&none.lakes)]);
        assert_eq!(
            seen,
            [
                11, 3, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 0, -1, -2, 0, 0, 1, 42, 0, 0
            ]
        );
    }
}
