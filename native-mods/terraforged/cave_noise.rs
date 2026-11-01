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

use super::terraforged::{Module, cell_vector, floor, value_coord_2d};

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

    pub fn height(&self, _seed: i32, x: i32, z: i32) -> i32 {
        scale_value(x, z, 1.0, self.min_y, self.range_y, &self.elevation)
    }

    pub fn cavern_size(&self, _seed: i32, x: i32, z: i32, modifier: f32) -> i32 {
        scale_value(x, z, modifier, 0, self.size, &self.shape)
    }

    pub fn floor_depth(&self, _seed: i32, x: i32, z: i32, size: i32) -> i32 {
        scale_value(x, z, 1.0, 0, size, &self.floor)
    }
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

    pub fn value(self, _seed: i32, mut x: f32, mut z: f32) -> f32 {
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
                let vector = cell_vector(self.seed, candidate_x, candidate_z);
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

fn scale_value(x: i32, z: i32, modifier: f32, minimum: i32, range: i32, noise: &Module) -> i32 {
    if range <= 0 {
        return 0;
    }
    minimum.wrapping_add(floor(
        noise.value(x as f32, z as f32) * range as f32 * modifier,
    ))
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
        assert_eq!(cave.height(91, 12, -8), 16);
        assert_eq!(cave.cavern_size(91, 12, -8, 0.5), 11);
        assert_eq!(cave.floor_depth(91, 12, -8, 11), 2);
        assert_eq!(cave.floor_depth(91, 12, -8, 0), 0);
    }

    #[test]
    fn unique_cave_distribution_matches_terraforged_source() {
        let cave = UniqueCaveDistributor::new(123, 0.01, 0.75, 1.0);
        let points = [
            (0.0, 0.0),
            (12.5, -81.25),
            (-450.0, 991.75),
            (2048.0, -4096.0),
        ];
        let values: Vec<_> = points
            .map(|(x, z)| cave.value(2132, x, z).to_bits())
            .into_iter()
            .collect();
        assert_eq!(values, [0x3e57285c, 0x3e4a155c, 0x3e0fdbd8, 0x3eebde26]);
        assert_eq!(
            UniqueCaveDistributor::new(123, 0.01, 0.75, 0.1)
                .value(2132, 0.0, 0.0)
                .to_bits(),
            0
        );
    }
}
