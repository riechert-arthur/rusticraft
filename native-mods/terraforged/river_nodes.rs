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
    terraforged::Interpolation,
    terraforged_engine::{Terrain, TerrainRegistry},
};

#[derive(Clone, Copy, Debug, PartialEq)]
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
    pub fn projection(self, x: f32, y: f32) -> f32 {
        let delta_x = self.bx - self.ax;
        let delta_y = self.by - self.ay;
        let value = ((x - self.ax) * delta_x + (y - self.ay) * delta_y)
            / (delta_x * delta_x + delta_y * delta_y);
        clamp(value, 0.0, 1.0)
    }

    pub fn distance_squared(self, x: f32, y: f32, projection: f32) -> f32 {
        let pad = 0.05;
        let alpha = map(projection, pad, 1.0 - pad, 1.0 - pad * 2.0);
        let alpha = if alpha < 0.5 {
            alpha / 0.5
        } else {
            (1.0 - alpha) / 0.5
        };
        let alpha = Interpolation::Curve3.apply(alpha) * self.displacement;
        let point_x = self.x(projection) - (self.by - self.ay) * alpha;
        let point_y = self.y(projection) + (self.bx - self.ax) * alpha;
        let delta_x = x - point_x;
        let delta_y = y - point_y;
        delta_x * delta_x + delta_y * delta_y
    }

    pub fn distance(self, x: f32, y: f32, projection: f32) -> f32 {
        (self.distance_squared(x, y, projection) as f64).sqrt() as f32
    }

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

pub struct CarverSample {
    pub river: NodeSample,
    pub lake: NodeSample,
}

impl CarverSample {
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

#[allow(clippy::manual_clamp)]
pub(crate) fn clamp(value: f32, minimum: f32, maximum: f32) -> f32 {
    if value < minimum {
        minimum
    } else if value > maximum {
        maximum
    } else {
        value
    }
}

pub(crate) fn map(value: f32, minimum: f32, maximum: f32, range: f32) -> f32 {
    let delta = clamp(value, minimum, maximum) - minimum;
    if delta > range { 1.0 } else { delta / range }
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

    #[test]
    fn sample_lifecycle_preserves_java_sentinels() {
        let mut samples = CarverSample::new(&TerrainRegistry::default());
        assert!(samples.river.is_invalid());
        assert!(samples.lake.is_invalid());
        samples.reset();
        assert_eq!(samples.river.distance, f32::MAX);
        assert!(!samples.lake.is_invalid());
        samples.river.level = 0.75;
        samples.river.invalidate();
        assert!(samples.river.is_invalid());
        assert_eq!(samples.river.level, 0.75);
        samples.reset();
        assert_eq!(samples.river.level, 0.0);
        let mut pieces = RiverPieces::default();
        for _ in 0..8 {
            pieces.rivers.push(node());
        }
        pieces.lakes.push(node());
        assert_eq!(pieces.rivers.len(), 8);
        pieces.reset();
        assert!(pieces.rivers.is_empty());
        assert!(pieces.lakes.is_empty());
    }
}
