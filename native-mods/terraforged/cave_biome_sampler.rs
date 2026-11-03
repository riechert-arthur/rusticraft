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
    terraforged::simplex, terraforged_cave_noise::CaveType,
    terraforged_terrain_blend_util::WeightMap,
};
use std::sync::Arc;

pub const OFFSET: i32 = 124_897;

pub struct CaveBiomeSampler<T> {
    scale: i32,
    frequency: f32,
    global: WeightMap<T>,
    unique: WeightMap<T>,
}

impl<T> CaveBiomeSampler<T> {
    pub fn new(scale: i32, global: Vec<Arc<T>>, special: Vec<Arc<T>>) -> Self {
        Self {
            scale,
            frequency: 1.0 / scale as f32,
            global: equal_weights(global),
            unique: equal_weights(special),
        }
    }

    pub const fn scale(&self) -> i32 {
        self.scale
    }

    pub const fn frequency(&self) -> f32 {
        self.frequency
    }

    /// Whether a cave may hold `biome` (it is in the global or the unique pool).
    pub fn may_hold(&self, biome: &T) -> bool
    where
        T: PartialEq,
    {
        self.global
            .values()
            .iter()
            .chain(self.unique.values())
            .any(|value| **value == *biome)
    }

    pub fn get_underground_biome(
        &self,
        seed: i32,
        x: i32,
        z: i32,
        cave_type: CaveType,
    ) -> Option<&Arc<T>> {
        let noise = sample(seed.wrapping_add(OFFSET), x, z, self.frequency);
        match cave_type {
            CaveType::Global => self.global.get_value(noise),
            CaveType::Unique => self.unique.get_value(noise),
        }
    }
}

pub fn sample(seed: i32, x: i32, z: i32, frequency: f32) -> f32 {
    let nx = x as f32 * frequency;
    let nz = z as f32 * frequency;
    ((1.0 + simplex(nx, nz, seed)) * 0.5).clamp(0.0, 1.0)
}

fn equal_weights<T>(values: Vec<Arc<T>>) -> WeightMap<T> {
    let weights = vec![1.0; values.len()];
    WeightMap::new(values, &weights)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choices_match_original_java_sampler() {
        let sampler = CaveBiomeSampler::new(
            800,
            vec![Arc::new(9)],
            vec![Arc::new(0), Arc::new(1), Arc::new(2)],
        );
        for (seed, x, z, bits, choice) in [
            (0i32, 0, 0, 1_056_964_608, 1),
            (0, 1024, -768, 1_060_992_068, 2),
            (123_456_789, -2048, 4096, 1_055_041_553, 1),
            (-987_654_321, 777, -333, 1_047_861_866, 0),
        ] {
            assert_eq!(
                sample(seed.wrapping_add(OFFSET), x, z, sampler.frequency()).to_bits(),
                bits
            );
            assert_eq!(
                sampler
                    .get_underground_biome(seed, x, z, CaveType::Unique)
                    .map(|value| **value),
                Some(choice)
            );
            assert_eq!(
                sampler
                    .get_underground_biome(seed, x, z, CaveType::Global)
                    .map(|value| **value),
                Some(9)
            );
        }
    }

    #[test]
    fn empty_pools_remain_independent() {
        let sampler = CaveBiomeSampler::new(800, vec![Arc::new(9)], Vec::new());
        assert!(
            sampler
                .get_underground_biome(0, 0, 0, CaveType::Unique)
                .is_none()
        );
        assert_eq!(
            sampler
                .get_underground_biome(0, 0, 0, CaveType::Global)
                .map(|value| **value),
            Some(9)
        );
        let sampler = CaveBiomeSampler::new(800, Vec::new(), vec![Arc::new(2)]);
        assert!(
            sampler
                .get_underground_biome(0, 0, 0, CaveType::Global)
                .is_none()
        );
        assert_eq!(
            sampler
                .get_underground_biome(0, 0, 0, CaveType::Unique)
                .map(|value| **value),
            Some(2)
        );
    }
}
