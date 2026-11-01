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

use super::terraforged::Module;

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
    Sum {
        initial: f32,
        rules: Vec<Self>,
        amounts: Vec<f32>,
    },
}

impl Viability {
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
                let scale = scaler(context);
                let minimum = base + min_offset * scale;
                let midpoint = base + mid_offset * scale;
                let maximum = base + max_offset * scale;
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
                let gradient = context.normalized_gradient(x, z, normalize * scaler(context));
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
            } => rules
                .iter()
                .zip(amounts)
                .fold(*initial, |sum, (rule, weight)| {
                    sum + rule.fitness(x, z, context) * weight
                })
                .clamp(0.0, 1.0),
        }
    }

    pub fn multiply(self, mut others: Vec<Self>) -> Self {
        others.push(self);
        Self::Multiply(others)
    }

    pub fn sum(initial: f32) -> SumBuilder {
        SumBuilder {
            initial,
            rules: Vec::new(),
            amounts: Vec::new(),
        }
    }
}

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

pub fn falloff(value: f32, maximum: f32) -> f32 {
    if value < maximum {
        1.0 - value / maximum
    } else {
        0.0
    }
}

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

fn scaler(context: &impl ViabilityContext) -> f32 {
    context.max_y() as f32 / 255.0
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Context;

    impl ViabilityContext for Context {
        fn seed(&self) -> i32 {
            17
        }

        fn edge(&self) -> bool {
            true
        }

        fn max_y(&self) -> i32 {
            320
        }

        fn base_height(&self, _x: i32, _z: i32) -> i32 {
            64
        }

        fn height(&self, _x: i32, _z: i32) -> i32 {
            65
        }

        fn normalized_gradient(&self, _x: i32, _z: i32, norm: f32) -> f32 {
            (0.3 * norm).clamp(0.0, 1.0)
        }

        fn river(&self, _x: i32, _z: i32) -> f32 {
            0.35
        }

        fn climate_shape(&self, _seed: i32, _x: i32, _z: i32) -> f32 {
            0.2
        }
    }

    #[test]
    fn viability_rules_match_terraforged_source() {
        let context = Context;
        let height = Viability::Height {
            min_offset: 0.0,
            mid_offset: 0.5,
            max_offset: 1.0,
        };
        let saturation = Viability::Saturation { min: 0.2, max: 0.8 };
        let slope = Viability::Slope {
            normalize: 1.5,
            max: 0.8,
        };
        let edge = Viability::BiomeEdge { distance: 0.75 };
        assert_eq!(height.fitness(0, 0, &context).to_bits(), 0x3f17ffda);
        assert_eq!(saturation.fitness(0, 0, &context).to_bits(), 0x3f3fffff);
        assert_eq!(slope.fitness(0, 0, &context).to_bits(), 0x3f34b4b6);
        assert_eq!(edge.fitness(0, 0, &context).to_bits(), 0x3f3bbbbc);

        let sum = Viability::sum(0.1)
            .with(0.6, edge.clone())
            .with(-0.2, Viability::Noise(Module::constant(0.75)))
            .build();
        assert_eq!(sum.fitness(0, 0, &context).to_bits(), 0x3ec7ae15);
        let multiplied = edge.multiply(vec![saturation, slope]);
        assert_eq!(multiplied.fitness(0, 0, &context).to_bits(), 0x3ec6c6c8);
    }

    #[test]
    fn viability_falloff_matches_terraforged_source() {
        assert_eq!(falloff(0.25, 1.0).to_bits(), 0x3f400000);
        assert_eq!(falloff(1.0, 1.0).to_bits(), 0);
        assert_eq!(ranged_falloff(0.25, 0.0, 0.5, 1.0).to_bits(), 0x3f000000);
        assert_eq!(ranged_falloff(0.75, 0.0, 0.5, 1.0).to_bits(), 0x3f000000);
    }
}
