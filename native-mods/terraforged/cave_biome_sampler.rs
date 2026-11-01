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
pub const MASK_OFFSET: i32 = 987_541;
pub const MASK_FREQUENCY: f32 = 1.0 / 1600.0;
pub const MASK_THRESHOLD: f32 = 0.62;

pub struct CaveBiomeSampler<T> {
    scale: i32,
    frequency: f32,
    global: WeightMap<T>,
    unique: WeightMap<T>,
}

impl<T> CaveBiomeSampler<T> {
    pub fn new(scale: i32, global: Vec<Arc<T>>, mut special: Vec<Arc<T>>) -> Self {
        if special.is_empty() {
            special = global.clone();
        }
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

    pub fn get_layer_biome(&self, seed: i32, x: i32, z: i32) -> Option<&Arc<T>> {
        let mask = sample(seed.wrapping_add(MASK_OFFSET), x, z, MASK_FREQUENCY);
        if mask < MASK_THRESHOLD {
            None
        } else {
            self.get_underground_biome(seed, x, z, CaveType::Unique)
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
    fn samples_and_layer_choices_match_current_java_source() {
        let sampler = CaveBiomeSampler::new(
            800,
            vec![Arc::new(9)],
            vec![Arc::new(0), Arc::new(1), Arc::new(2)],
        );
        assert_eq!(sampler.scale(), 800);
        assert_eq!(sampler.frequency().to_bits(), 983_815_946);
        let fixtures: [(i32, i32, i32, u32, u32, Option<i32>); 4] = [
            (0, 0, 0, 1_056_964_608, 1_056_964_608, None),
            (0, 1024, -768, 1_060_992_068, 1_042_551_020, None),
            (
                123_456_789,
                -2048,
                4096,
                1_055_041_553,
                1_060_087_566,
                Some(1),
            ),
            (-987_654_321, 777, -333, 1_047_861_866, 1_038_883_312, None),
        ];
        for (seed, x, z, choice_bits, mask_bits, layer) in fixtures {
            assert_eq!(
                sample(seed.wrapping_add(OFFSET), x, z, sampler.frequency()).to_bits(),
                choice_bits
            );
            assert_eq!(
                sample(seed.wrapping_add(MASK_OFFSET), x, z, MASK_FREQUENCY).to_bits(),
                mask_bits
            );
            assert_eq!(
                sampler.get_layer_biome(seed, x, z).map(|value| **value),
                layer
            );
        }
    }

    #[test]
    fn empty_special_set_falls_back_to_global_set() {
        let global = Arc::new(7);
        let sampler = CaveBiomeSampler::new(800, vec![Arc::clone(&global)], Vec::new());
        assert!(Arc::ptr_eq(
            sampler
                .get_underground_biome(0, 0, 0, CaveType::Unique)
                .unwrap(),
            &global
        ));
    }
}
