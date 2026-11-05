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
    terraforged::{Module, clamp},
    terraforged_terrain_data::TerrainData,
};

/// TerraForged 1.18.2-0.3.1's `Viability.Context`, flattened to the queries the rules make: `max_y` is
/// `getLevels().maxY`, `base_height`/`height` are `getTerrain().getBaseHeight`/`getHeight(x, z)`,
/// `normalized_gradient` is `getTerrain().getGradient(x, z, norm)`, `river` is `getTerrain().getRiver().get(x, z)`
/// and `climate_shape` is `getClimateSampler().getShape(x, z)`.
///
/// `seed` (and `climate_shape`'s seed argument) come from the newer TerraForged source, whose context carries the
/// world seed and whose `IBiomeSampler.getShape` takes it. The jar has neither: the seed is kept for the
/// runtime-seed layers and is unused until they land; jar parity means `climate_shape` ignores it.
pub trait ViabilityContext {
    fn seed(&self) -> i32;
    fn edge(&self) -> bool;
    fn max_y(&self) -> i32;
    fn base_height(&self, x: i32, z: i32) -> i32;
    fn height(&self, x: i32, z: i32) -> i32;
    fn normalized_gradient(&self, x: i32, z: i32, norm: f32) -> f32;
    fn river(&self, x: i32, z: i32) -> f32;
    fn climate_shape(&self, seed: i32, x: i32, z: i32) -> f32;
}

/// The jar's `ViabilityContext` class over one chunk's [`TerrainData`] (its `terrainData` future, already
/// joined): no biome edges, levels from the terrain data. Its biome sampler is only read by `BiomeEdgeViability`
/// when `edge()` is true, which it never is here, so it is not held (`IBiomeSampler` is not ported yet) and
/// `climate_shape` returns 0. The seed is 0 (the jar's class has none).
pub struct TerrainViabilityContext<'a>(pub &'a TerrainData);

impl ViabilityContext for TerrainViabilityContext<'_> {
    fn seed(&self) -> i32 {
        0
    }

    fn edge(&self) -> bool {
        false
    }

    fn max_y(&self) -> i32 {
        self.0.levels().max_y
    }

    fn base_height(&self, x: i32, z: i32) -> i32 {
        self.0.base_height_at(x, z)
    }

    fn height(&self, x: i32, z: i32) -> i32 {
        self.0.height_at(x, z)
    }

    fn normalized_gradient(&self, x: i32, z: i32, norm: f32) -> f32 {
        self.0.normalized_gradient(x, z, norm)
    }

    fn river(&self, x: i32, z: i32) -> f32 {
        self.0.river().get(x, z)
    }

    fn climate_shape(&self, _seed: i32, _x: i32, _z: i32) -> f32 {
        0.0
    }
}

/// TerraForged 1.18.2-0.3.1's viability rules (`worldgen.biome.viability`): `Viability.NONE` and the
/// `BiomeEdgeViability`, `HeightViability`, `MultViability`, `NoiseViability`, `SaturationViability`,
/// `SlopeViability` and `SumViability` records. Comparisons keep Java's NaN behaviour (a NaN never passes a
/// threshold test). Their Cereal specs and `ViabilityCodec` are not ported.
#[derive(Clone)]
pub enum Viability {
    None,
    BiomeEdge {
        distance: f32,
    },
    Height {
        min_offset: f32,
        mid_offset: f32,
        max_offset: f32,
    },
    Multiply(Vec<Self>),
    Noise(Module),
    Saturation {
        min: f32,
        max: f32,
    },
    Slope {
        normalize: f32,
        max: f32,
    },
    /// Weighted sum: `amounts[i]` weighs `rules[i]`. Extra amounts are ignored; fewer amounts than rules panic
    /// where Java throws `ArrayIndexOutOfBoundsException`.
    Sum {
        initial: f32,
        rules: Vec<Self>,
        amounts: Vec<f32>,
    },
}

impl Viability {
    /// `getFitness(x, z, context)`.
    pub fn fitness(&self, x: i32, z: i32, context: &impl ViabilityContext) -> f32 {
        match self {
            Self::None => 1.0,
            Self::BiomeEdge { distance } => {
                if !context.edge() {
                    return 0.0;
                }
                let edge = context.climate_shape(context.seed(), x, z);
                if edge > *distance {
                    0.0
                } else {
                    1.0 - edge / distance
                }
            }
            Self::Height {
                min_offset,
                mid_offset,
                max_offset,
            } => {
                let base = context.base_height(x, z) as f32;
                let height = context.height(x, z) as f32;
                let scale = scaler(context.max_y());
                let minimum = base + min_offset * scale;
                let midpoint = base + mid_offset * scale;
                let maximum = base + max_offset * scale;
                // Outside the band the jar returns 1, not 0.
                if height < minimum || height > maximum {
                    1.0
                } else if height < midpoint {
                    (midpoint - height) / (midpoint - minimum)
                } else {
                    (height - midpoint) / (maximum - midpoint)
                }
            }
            Self::Multiply(rules) => rules
                .iter()
                .fold(1.0, |value, rule| value * rule.fitness(x, z, context)),
            Self::Noise(noise) => noise.value(x as f32, z as f32),
            Self::Saturation { min, max } => {
                let saturation = 1.0 - context.river(x, z);
                if saturation < *min {
                    0.0
                } else if saturation > *max {
                    1.0
                } else {
                    (saturation - min) / (max - min)
                }
            }
            Self::Slope { normalize, max } => {
                let gradient =
                    context.normalized_gradient(x, z, normalize * scaler(context.max_y()));
                if gradient >= *max {
                    1.0
                } else {
                    gradient / max
                }
            }
            Self::Sum {
                initial,
                rules,
                amounts,
            } => {
                let mut sum = *initial;
                for (i, rule) in rules.iter().enumerate() {
                    sum += rule.fitness(x, z, context) * amounts[i];
                }
                clamp(sum, 0.0, 1.0)
            }
        }
    }

    /// `mult(others...)`: a `MultViability` over `others` followed by `self`.
    pub fn multiply(self, mut others: Vec<Self>) -> Self {
        others.push(self);
        Self::Multiply(others)
    }

    /// `new SaturationViability(max)`: the minimum is 0.
    pub const fn saturation(max: f32) -> Self {
        Self::Saturation { min: 0.0, max }
    }

    /// `SumViability.builder(initial)`.
    pub fn sum(initial: f32) -> SumBuilder {
        SumBuilder {
            initial,
            rules: Vec::new(),
            amounts: Vec::new(),
        }
    }
}

/// `SumViability.Builder`.
pub struct SumBuilder {
    initial: f32,
    rules: Vec<Viability>,
    amounts: Vec<f32>,
}

impl SumBuilder {
    pub fn with(mut self, weight: f32, viability: Viability) -> Self {
        self.rules.push(viability);
        self.amounts.push(weight);
        self
    }

    pub fn build(self) -> Viability {
        Viability::Sum {
            initial: self.initial,
            rules: self.rules,
            amounts: self.amounts,
        }
    }
}

/// `Viability.getFallOff(value, max)`.
pub fn falloff(value: f32, maximum: f32) -> f32 {
    if value < maximum {
        1.0 - value / maximum
    } else {
        0.0
    }
}

/// `Viability.getFallOff(value, min, mid, max)`.
pub fn ranged_falloff(value: f32, minimum: f32, midpoint: f32, maximum: f32) -> f32 {
    if value < minimum {
        0.0
    } else if value < midpoint {
        (value - minimum) / (midpoint - minimum)
    } else if value < maximum {
        1.0 - (value - midpoint) / (maximum - midpoint)
    } else {
        0.0
    }
}

/// `getScaler(levels)`: `levels.maxY / 255F`.
fn scaler(max_y: i32) -> f32 {
    max_y as f32 / 255.0
}

#[cfg(test)]
mod tests {
    use super::super::{
        terraforged::{Interpolation, Source, SourceConfig, SourceKind},
        terraforged_engine::TerrainRegistry,
        terraforged_terrain_data::{NoiseData, NoiseSample},
        terraforged_terrain_levels::TerrainLevels,
    };
    use super::*;
    use std::fmt::Write;

    /// `BiomeViabilityOracle.java` (`.local/terraforged/oracles/biome_viability/`): TerraForged 1.18.2-0.3.1's own
    /// viability bytecode; each section line is `<name> <count> <FNV-1a of the canonical fitness bits>`.
    const ORACLE: &str = "\
D 3f17ffda 3f34b4b6 3f3fffff 3f3bbbbc 3f400000 3f800000 3ec6c6c8 3f800000 3f17ffda 7fc00000 3e1b0bb1 3ec7ae15 \
00000000 7fc00000 7fc00000 00000000 80000000 7fc00000 3f732715 3f800000 3f800000 3f40c0c2 3f800000
E 968 fd1abfc941dcc340
H 16016 7b5fb53bf9c61a74
S 1540 e0cd533f4561be78
R 660 a8a92ab594396ed9
N 40 db81b96348c0ded
M 26496 3bd06945531bbc1d
F 682 cc72beca912fec5b
T 44850 a46152e71d391cc1
";

    #[rustfmt::skip]
    const F: [f32; 22] = [
        0.0, -0.0, 1.0, -1.0, 0.2, 0.35, 0.5, 0.75, 0.8, 0.799_999_95, 0.800_000_07, 1.000_000_1, 2.0, -0.5,
        f32::from_bits(1), -f32::from_bits(1), f32::MAX, INF, -INF, NAN, 1e-30, 255.0,
    ];
    const MAX_Y: [i32; 7] = [128, 255, 320, 480, 4064, 100, 5000];
    const BASES: [i32; 8] = [0, 64, 128, -64, 62, i32::MAX, i32::MIN, 16_777_217];
    #[rustfmt::skip]
    const DELTAS: [i32; 26] = [
        0, 1, -1, 2, -2, 31, 32, 33, 63, 64, 65, -64, 127, 128, 129, 159, 160, 161, 255, 256, 320, 480, -480, 1000,
        i32::MAX, i32::MIN,
    ];
    const NAN: f32 = f32::NAN;
    const INF: f32 = f32::INFINITY;
    #[rustfmt::skip]
    const HEIGHT: [[f32; 3]; 11] = [
        [0.0, 0.5, 1.0], [0.1, 0.2, 0.3], [-0.5, 0.0, 0.5], [0.25, 0.25, 0.25], [1.0, 0.5, 0.0], [NAN, 0.5, 1.0],
        [0.0, NAN, 1.0], [0.0, 0.5, INF], [-INF, 0.0, INF], [-0.0, 0.0, 0.0], [0.0, 0.5, 1e30],
    ];
    #[rustfmt::skip]
    const SLOPE: [[f32; 2]; 10] = [
        [1.0, 0.8], [1.5, 0.8], [0.0, 1.0], [1.0, 0.0], [1.0, -0.0], [1.0, NAN], [NAN, 1.0], [INF, 0.5],
        [1.0, INF], [2.0, -0.5],
    ];
    #[rustfmt::skip]
    const SATURATION: [[f32; 2]; 8] = [
        [0.2, 0.8], [0.0, 1.0], [0.5, 0.5], [0.8, 0.2], [NAN, 1.0], [0.0, NAN], [-INF, INF], [-0.0, 0.0],
    ];
    #[rustfmt::skip]
    const POINTS: [(i32, i32); 8] = [
        (0, 0), (12, -8), (-1000, 777), (i32::MAX, i32::MIN), (i32::MIN, i32::MAX), (16_777_217, -16_777_217),
        (123_456_789, -987_654_321), (-1, -1),
    ];

    /// The oracle's `Ctx` over a `StubTerrain`: fixed heights, an unclamped `gradient * norm` (so NaN and
    /// infinities reach the rules), a constant river and climate shape.
    struct Stub {
        edge: bool,
        max_y: i32,
        base: i32,
        height: i32,
        gradient: f32,
        river: f32,
        shape: f32,
    }

    impl ViabilityContext for Stub {
        fn seed(&self) -> i32 {
            0
        }
        fn edge(&self) -> bool {
            self.edge
        }
        fn max_y(&self) -> i32 {
            self.max_y
        }
        fn base_height(&self, _x: i32, _z: i32) -> i32 {
            self.base
        }
        fn height(&self, _x: i32, _z: i32) -> i32 {
            self.height
        }
        fn normalized_gradient(&self, _x: i32, _z: i32, norm: f32) -> f32 {
            self.gradient * norm
        }
        fn river(&self, _x: i32, _z: i32) -> f32 {
            self.river
        }
        fn climate_shape(&self, _seed: i32, _x: i32, _z: i32) -> f32 {
            self.shape
        }
    }

    /// `ctx(edge, maxY, base, top, gradient, river, shape)`: `maxY` goes through `TerrainLevels`' clamps.
    fn ctx(
        edge: bool,
        max_y: i32,
        base: i32,
        height: i32,
        gradient: f32,
        river: f32,
        shape: f32,
    ) -> Stub {
        Stub {
            edge,
            max_y: levels(max_y).max_y,
            base,
            height,
            gradient,
            river,
            shape,
        }
    }

    fn levels(max_y: i32) -> TerrainLevels {
        TerrainLevels::new(false, 1.0, -64, max_y, 128, 62, 22)
    }

    #[derive(Default)]
    struct Hash {
        hash: u64,
        count: usize,
        out: String,
    }

    impl Hash {
        const BASIS: u64 = 0xcbf2_9ce4_8422_2325;

        fn add(&mut self, value: f32) {
            if self.count == 0 {
                self.hash = Self::BASIS;
            }
            self.hash = bits(value)
                .to_le_bytes()
                .iter()
                .fold(self.hash, |hash, byte| {
                    (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
                });
            self.count += 1;
        }

        fn section(&mut self, name: &str) {
            let hash = if self.count == 0 {
                Self::BASIS
            } else {
                self.hash
            };
            writeln!(self.out, "{name} {} {hash:x}", self.count).unwrap();
            self.count = 0;
        }
    }

    /// `Float.floatToIntBits`: every NaN is 7fc00000.
    fn bits(value: f32) -> u32 {
        if value.is_nan() {
            0x7fc0_0000
        } else {
            value.to_bits()
        }
    }

    fn simplex(seed: i32, frequency: f32, octaves: usize) -> Module {
        Module::source(Source::new(
            SourceKind::Simplex,
            SourceConfig {
                seed,
                octaves,
                gain: 0.5,
                frequency,
                lacunarity: 2.0,
                interpolation: Interpolation::Curve3,
            },
        ))
    }

    fn height(min: f32, mid: f32, max: f32) -> Viability {
        Viability::Height {
            min_offset: min,
            mid_offset: mid,
            max_offset: max,
        }
    }

    /// `BiomeViabilityOracle.composites`.
    fn composites() -> Vec<Viability> {
        let r1 = height(0.0, 0.5, 1.0);
        let r2 = Viability::Slope {
            normalize: 1.5,
            max: 0.8,
        };
        let r3 = Viability::Saturation { min: 0.2, max: 0.8 };
        let r4 = Viability::BiomeEdge { distance: 0.75 };
        let r5 = Viability::Noise(Module::constant(0.75));
        let r6 = Viability::None;
        let m1 = r4.clone().multiply(vec![r3.clone(), r2.clone()]);
        let sum1 = Viability::sum(0.1)
            .with(0.6, r4.clone())
            .with(-0.2, r5.clone())
            .build();
        vec![
            r1.clone(),
            r2.clone(),
            r3.clone(),
            r4,
            r5,
            r6.clone(),
            m1.clone(),
            Viability::Multiply(Vec::new()),
            r1.clone().multiply(Vec::new()),
            Viability::Multiply(vec![r1.clone(), Viability::Noise(Module::constant(NAN))]),
            m1.clone().multiply(vec![sum1.clone(), r6.clone()]),
            sum1,
            Viability::sum(0.0).build(),
            Viability::sum(NAN).with(1.0, r6.clone()).build(),
            Viability::sum(0.5).with(NAN, r1.clone()).build(),
            Viability::sum(-0.0)
                .with(1.0, r6.clone())
                .with(-1.0, r6.clone())
                .build(),
            Viability::sum(-0.0).build(),
            Viability::sum(2.0)
                .with(INF, r6.clone())
                .with(-INF, r6)
                .build(),
            Viability::Sum {
                initial: 0.3,
                rules: vec![r1, r2.clone()],
                amounts: vec![0.5, 0.5, 9.0],
            },
            Viability::sum(0.9)
                .with(0.5, r3)
                .with(1.0, r2)
                .with(-0.25, m1)
                .build(),
            Viability::saturation(0.6),
            Viability::Slope {
                normalize: 1.0,
                max: 0.5,
            },
            height(0.1, 0.2, 0.3),
        ]
    }

    /// Every section of `BiomeViabilityOracle`, in its order.
    fn oracle_output() -> String {
        let mut out = String::from("D");
        let basic = ctx(true, 320, 64, 65, 0.3, 0.35, 0.2);
        let all = composites();
        for rule in &all {
            write!(out, " {:08x}", bits(rule.fitness(0, 0, &basic))).unwrap();
        }
        out.push('\n');
        let mut h = Hash {
            out,
            ..Hash::default()
        };

        for edge in [false, true] {
            for distance in F {
                for shape in F {
                    let rule = Viability::BiomeEdge { distance };
                    h.add(rule.fitness(3, -7, &ctx(edge, 255, 0, 0, 0.0, 0.0, shape)));
                }
            }
        }
        h.section("E");

        for max_y in MAX_Y {
            for [min, mid, max] in HEIGHT {
                let rule = height(min, mid, max);
                for base in BASES {
                    for dy in DELTAS {
                        let context = ctx(false, max_y, base, base.wrapping_add(dy), 0.0, 0.0, 0.0);
                        h.add(rule.fitness(5, 9, &context));
                    }
                }
            }
        }
        h.section("H");

        for max_y in MAX_Y {
            for [normalize, max] in SLOPE {
                let rule = Viability::Slope { normalize, max };
                for gradient in F {
                    h.add(rule.fitness(-2, 4, &ctx(false, max_y, 0, 0, gradient, 0.0, 0.0)));
                }
            }
        }
        h.section("S");

        for [min, max] in SATURATION {
            let rule = Viability::Saturation { min, max };
            for river in F {
                h.add(rule.fitness(17, -33, &ctx(false, 255, 0, 0, 0.0, river, 0.0)));
            }
        }
        for max in F {
            for river in F {
                let context = ctx(false, 255, 0, 0, 0.0, river, 0.0);
                h.add(Viability::saturation(max).fitness(1, 1, &context));
            }
        }
        h.section("R");

        let modules = [
            Module::constant(0.75),
            Module::constant(-0.0),
            Module::constant(NAN),
            simplex(1337, 0.01, 3),
            simplex(-5, 1.0, 1),
        ];
        for module in modules {
            let rule = Viability::Noise(module);
            for (x, z) in POINTS {
                h.add(rule.fitness(x, z, &basic));
            }
        }
        h.section("N");

        for max_y in [255, 320, 4064] {
            for (base, top) in [(64, 65), (64, 200), (0, -1), (64, 192)] {
                for gradient in [0.3, 0.6, NAN, 1.0] {
                    for river in [0.35, 0.9, NAN, -0.0] {
                        for shape in [0.2, 1.0, NAN] {
                            for edge in [false, true] {
                                let context = ctx(edge, max_y, base, top, gradient, river, shape);
                                for rule in &all {
                                    h.add(rule.fitness(7, 7, &context));
                                }
                            }
                        }
                    }
                }
            }
        }
        h.section("M");

        for value in F {
            for max in F {
                h.add(falloff(value, max));
            }
        }
        #[rustfmt::skip]
        let triples = [
            [0.0, 0.5, 1.0], [0.2, 0.2, 0.8], [0.2, 0.8, 0.8], [1.0, 0.5, 0.0], [NAN, 0.5, 1.0], [0.0, NAN, 1.0],
            [0.0, 0.5, NAN], [-INF, 0.0, INF], [-0.0, 0.0, 0.0],
        ];
        for value in F {
            for [min, mid, max] in triples {
                h.add(ranged_falloff(value, min, mid, max));
            }
        }
        h.section("F");

        let terrains = TerrainRegistry::new();
        let terrain_levels = [TerrainLevels::unconfigured(), levels(320), levels(4064)];
        for kind in 0..2 {
            for level in terrain_levels {
                let mut data = NoiseData::new(&terrains);
                let mut sample = NoiseSample::new(&terrains);
                for z in -1..=16 {
                    for x in -1..=16 {
                        sample.reset(&terrains);
                        if kind == 0 {
                            sample.height_noise = 0.25 + (x * 7 + z * 3) as f32 / 512.0;
                            sample.base_noise = (x - z) as f32 / 64.0 + 0.5;
                            sample.river_noise = if (x + z) & 3 == 0 {
                                0.0
                            } else {
                                (x * z) as f32 / 300.0
                            };
                        } else {
                            sample.height_noise = ((x * x + 3 * z) % 17) as f32 / 16.0 - 0.03;
                            sample.base_noise = 0.2 + z as f32 / 40.0;
                            sample.river_noise = (x - z) as f32 / 20.0;
                            if x == 3 && z == 4 {
                                sample.height_noise = NAN;
                            }
                            if x == 5 && z == 5 {
                                sample.river_noise = INF;
                            }
                        }
                        data.set_noise(x, z, &sample);
                    }
                }
                let mut terrain = TerrainData::new(level);
                terrain.accept(&data);
                let context = TerrainViabilityContext(&terrain);
                for rule in &all {
                    for z in -1..=16 {
                        for x in -1..=16 {
                            h.add(rule.fitness(x, z, &context));
                        }
                    }
                    h.add(rule.fitness(i32::MAX, i32::MIN, &context));
                }
            }
        }
        h.section("T");
        h.out
    }

    #[test]
    fn viability_matches_terraforged_jar() {
        assert_eq!(oracle_output(), ORACLE);
    }

    #[test]
    #[should_panic]
    fn sum_with_missing_amount_panics_like_java() {
        let rule = Viability::Sum {
            initial: 0.0,
            rules: vec![Viability::None, Viability::None],
            amounts: vec![1.0],
        };
        rule.fitness(0, 0, &ctx(false, 255, 0, 0, 0.0, 0.0, 0.0));
    }

    #[test]
    fn mult_appends_self_last() {
        let rule = Viability::None.multiply(vec![Viability::BiomeEdge { distance: 1.0 }]);
        assert!(matches!(
            &rule,
            Viability::Multiply(rules) if matches!(rules[..], [Viability::BiomeEdge { .. }, Viability::None])
        ));
    }
}
