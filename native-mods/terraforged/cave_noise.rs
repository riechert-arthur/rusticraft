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

use super::terraforged::{
    Domain, Interpolation, Module, Source, SourceConfig, SourceKind, cell_vector, floor,
    value_coord_2d,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaveType {
    Global,
    Unique,
}

impl CaveType {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Global => "global",
            Self::Unique => "unique",
        }
    }

    pub fn for_name(name: &str) -> Option<Self> {
        if name.eq_ignore_ascii_case("global") {
            Some(Self::Global)
        } else if name.eq_ignore_ascii_case("unique") {
            Some(Self::Unique)
        } else {
            None
        }
    }
}

#[derive(Clone)]
pub struct NoiseCave {
    seed: i32,
    cave_type: CaveType,
    elevation: Module,
    shape: Module,
    floor: Module,
    size: i32,
    min_y: i32,
    max_y: i32,
    range_y: i32,
}

impl NoiseCave {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        seed: i32,
        cave_type: CaveType,
        elevation: Module,
        shape: Module,
        floor: Module,
        size: i32,
        min_y: i32,
        max_y: i32,
    ) -> Self {
        Self {
            seed,
            cave_type,
            elevation,
            shape,
            floor,
            size,
            min_y,
            max_y,
            range_y: max_y.wrapping_sub(min_y),
        }
    }

    pub fn with_seed(&self, seed: i64) -> Self {
        Self::new(
            self.seed,
            self.cave_type,
            self.elevation.with_seed(seed),
            self.shape.with_seed(seed),
            self.floor.with_seed(seed),
            self.size,
            self.min_y,
            self.max_y,
        )
    }

    pub const fn seed(&self) -> i32 {
        self.seed
    }

    pub const fn cave_type(&self) -> CaveType {
        self.cave_type
    }

    pub fn height(&self, seed: i32, x: i32, z: i32) -> Result<i32, &'static str> {
        scale_value(seed, x, z, 1.0, self.min_y, self.range_y, &self.elevation)
    }

    pub fn cavern_size(
        &self,
        seed: i32,
        x: i32,
        z: i32,
        modifier: f32,
    ) -> Result<i32, &'static str> {
        scale_value(seed, x, z, modifier, 0, self.size, &self.shape)
    }

    pub fn floor_depth(&self, seed: i32, x: i32, z: i32, size: i32) -> Result<i32, &'static str> {
        scale_value(seed, x, z, 1.0, 0, size, &self.floor)
    }
}

/// `NoiseCaveGenerator.createUniqueNoise(500, DENSITY)`: where unique caves form.
pub fn unique_cave_noise() -> Module {
    UniqueCaveDistributor::new(1_286_745, 1.0 / 500.0, 0.75, 0.05)
        .module()
        .clamp(Module::constant(0.2), Module::constant(1.0))
        .map(Module::constant(0.0), Module::constant(1.0))
        .warp(Domain::domain_warp(
            perlin(781_624, 30, 1),
            perlin(781_625, 30, 1),
            Module::constant(20.0),
        ))
}

/// `NoiseCaveGenerator.createBreachNoise(300, BREACH_THRESHOLD)`: where caves may open
/// towards the surface.
pub fn cave_breach_noise() -> Module {
    let threshold = 0.7_f32;
    builder_source(SourceKind::SimplexRidge, 1_567_328, 300, 2, 0.975)
        .clamp(
            Module::constant(threshold * 0.8),
            Module::constant(threshold),
        )
        .map(Module::constant(0.0), Module::constant(1.0))
}

/// `Source.build(seed, scale, octaves).build(Source.PERLIN)`.
fn perlin(seed: i32, scale: i32, octaves: usize) -> Module {
    builder_source(SourceKind::Perlin, seed, scale, octaves, 0.5)
}

/// Noise2D's `Builder` with its default lacunarity and interpolation.
fn builder_source(kind: SourceKind, seed: i32, scale: i32, octaves: usize, gain: f32) -> Module {
    Module::source(Source::new(
        kind,
        SourceConfig {
            seed,
            octaves,
            gain,
            frequency: 1.0 / scale as f32,
            lacunarity: 2.0,
            interpolation: Interpolation::Curve3,
        },
    ))
}

#[derive(Clone, Copy)]
pub struct UniqueCaveDistributor {
    seed: i32,
    frequency: f32,
    jitter: f32,
    density: f32,
}

impl UniqueCaveDistributor {
    pub const fn new(seed: i32, frequency: f32, jitter: f32, density: f32) -> Self {
        Self {
            seed,
            frequency,
            jitter,
            density,
        }
    }

    /// The distributor as a graph node, so it can be clamped, mapped and warped.
    pub fn module(self) -> Module {
        Module::seeded_custom(0.0, 1.0, move |seed, x, z| self.value(seed, x, z))
    }

    /// `getValue(int seed, ...)`: cell positions use the runtime seed, which shadows the
    /// field; the occupancy roll in `cell_value` uses the field.
    pub fn value(self, seed: i32, mut x: f32, mut z: f32) -> f32 {
        x *= self.frequency;
        z *= self.frequency;
        let max_x = floor(x).wrapping_add(1);
        let max_z = floor(z).wrapping_add(1);
        let mut cell_x = max_x.wrapping_sub(1);
        let mut cell_z = max_z.wrapping_sub(1);
        let mut nearest = f32::INFINITY;
        let mut nearest2 = f32::INFINITY;
        for candidate_z in max_z.wrapping_sub(2)..=max_z {
            let offset_x = (candidate_z & 1) as f32 * 0.5;
            for candidate_x in max_x.wrapping_sub(2)..=max_x {
                let vector = cell_vector(seed, candidate_x, candidate_z);
                let point_x = candidate_x as f32 + offset_x + vector[0] * self.jitter;
                let point_z = candidate_z as f32 + vector[1] * self.jitter;
                let delta_x = x - point_x;
                let delta_z = z - point_z;
                let distance = delta_x * delta_x + delta_z * delta_z;
                if distance < nearest {
                    nearest2 = nearest;
                    nearest = distance;
                    cell_x = candidate_x;
                    cell_z = candidate_z;
                } else if distance < nearest2 {
                    nearest2 = distance;
                }
            }
        }
        if self.cell_value(cell_x, cell_z) > self.density {
            0.0
        } else {
            1.0 - (nearest / nearest2).sqrt()
        }
    }

    fn cell_value(self, cell_x: i32, cell_z: i32) -> f32 {
        (1.0 + value_coord_2d(self.seed, cell_x, cell_z)) * 0.5
    }
}

fn scale_value(
    seed: i32,
    x: i32,
    z: i32,
    modifier: f32,
    minimum: i32,
    range: i32,
    noise: &Module,
) -> Result<i32, &'static str> {
    if range <= 0 {
        return Ok(0);
    }
    let value = noise.try_value_seed_first(seed, x as f32, z as f32)?;
    Ok(minimum.wrapping_add(floor(value * range as f32 * modifier)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cave_assets_match_terraforged_source() {
        assert_eq!(CaveType::for_name("GLOBAL"), Some(CaveType::Global));
        assert_eq!(CaveType::Unique.name(), "unique");
        assert_eq!(CaveType::for_name("missing"), None);

        let cave = NoiseCave::new(
            17,
            CaveType::Unique,
            Module::constant(0.5),
            Module::constant(0.75),
            Module::constant(0.25),
            30,
            -32,
            64,
        );
        assert_eq!(cave.seed(), 17);
        assert_eq!(cave.cave_type(), CaveType::Unique);
        assert_eq!(cave.height(91, 12, -8), Ok(16));
        assert_eq!(cave.cavern_size(91, 12, -8, 0.5), Ok(11));
        assert_eq!(cave.floor_depth(91, 12, -8, 11), Ok(2));
        assert_eq!(cave.floor_depth(91, 12, -8, 0), Ok(0));
    }

    // From tools/terraforged/TerraForgedNoiseCaveOracle.java: seed, x, z, unique, breach.
    const MODIFIERS: [(i32, i32, i32, u32, u32); 15] = [
        (0, 0, 0, 0, 1065353216),
        (0, 37, -91, 0, 0),
        (0, -1024, 4096, 0, 1040947585),
        (0, 12345, -6789, 0, 0),
        (0, -30001, 15003, 0, 0),
        (1337, 0, 0, 0, 1065353216),
        (1337, 37, -91, 0, 0),
        (1337, -1024, 4096, 0, 0),
        (1337, 12345, -6789, 0, 0),
        (1337, -30001, 15003, 0, 0),
        (-987654321, 0, 0, 0, 1065353216),
        (-987654321, 37, -91, 0, 0),
        (-987654321, -1024, 4096, 0, 0),
        (-987654321, 12345, -6789, 0, 0),
        (-987654321, -30001, 15003, 0, 0),
    ];

    // The oracle's first unique-cave hits along a fixed scan: seed, x, z, value.
    const UNIQUE_HITS: [(i32, i32, i32, u32); 12] = [
        (0, -887584, 292192, 1032954723),
        (0, -887487, 292131, 1060519800),
        (0, -887390, 292070, 1063312299),
        (0, -887293, 292009, 1053391849),
        (1337, -896217, 297621, 1025155803),
        (1337, -896120, 297560, 1037706053),
        (1337, -881570, 288410, 1042212703),
        (1337, -880600, 287800, 1049766814),
        (-987654321, -896314, 297682, 1058922596),
        (-987654321, -896217, 297621, 1065205434),
        (-987654321, -896120, 297560, 1059647356),
        (-987654321, -896023, 297499, 1049694601),
    ];

    #[test]
    fn cave_modifiers_match_terraforged_source() {
        let unique = unique_cave_noise();
        let breach = cave_breach_noise();
        for (seed, x, z, unique_bits, breach_bits) in MODIFIERS {
            let (x, z) = (x as f32, z as f32);
            let value = unique.try_value_seed_first(seed, x, z).unwrap();
            assert_eq!(value.to_bits(), unique_bits, "unique {seed} {x} {z}");
            let value = breach.try_value_seed_first(seed, x, z).unwrap();
            assert_eq!(value.to_bits(), breach_bits, "breach {seed} {x} {z}");
        }
        for (seed, x, z, bits) in UNIQUE_HITS {
            let value = unique
                .try_value_seed_first(seed, x as f32, z as f32)
                .unwrap();
            assert_eq!(value.to_bits(), bits, "unique hit {seed} {x} {z}");
        }
    }
}
