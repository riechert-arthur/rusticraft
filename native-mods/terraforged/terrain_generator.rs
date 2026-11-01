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
    terraforged_terrain_data::{NoiseData, TerrainData},
    terraforged_terrain_levels::{NoiseLevels, TerrainLevels},
};
use std::{collections::VecDeque, sync::Mutex};

struct PoolState<T> {
    size: i32,
    pool: VecDeque<T>,
}

pub struct ObjectPool<T, F> {
    capacity: i32,
    max_index: i32,
    factory: F,
    state: Mutex<PoolState<T>>,
}

impl<T, F: Fn() -> T> ObjectPool<T, F> {
    pub fn new(capacity: i32, factory: F) -> Self {
        let mut pool = VecDeque::new();
        let mut remaining = capacity;
        while remaining > 0 {
            pool.push_back(factory());
            remaining -= 1;
        }
        Self {
            capacity,
            max_index: capacity.wrapping_add(1),
            factory,
            state: Mutex::new(PoolState {
                size: capacity,
                pool,
            }),
        }
    }

    pub fn take(&self) -> T {
        let mut state = self.state.lock().unwrap();
        if state.pool.pop_front().is_some() {
            state.size = state.size.wrapping_sub(1);
        }
        drop(state);
        (self.factory)()
    }

    pub fn restore(&self, value: T) {
        let mut state = self.state.lock().unwrap();
        state.size = if state.size < self.capacity {
            state.size.wrapping_add(1)
        } else {
            self.max_index
        };
        if state.size < self.max_index {
            state.pool.push_back(value);
        }
    }

    pub fn for_cache_size(cache_size: i32, factory: F) -> Self {
        Self::new((cache_size as f32 * 1.75).floor() as i32, factory)
    }
}

pub trait NoiseGenerator: Send + Sync {
    fn levels(&self) -> NoiseLevels;

    fn terrain_levels(&self) -> TerrainLevels;

    fn height_noise(&self, seed: i32, x: i32, z: i32) -> f32;

    fn generate(&self, seed: i32, chunk_x: i32, chunk_z: i32, consumer: &mut dyn FnMut(&NoiseData));

    fn noise_coordinate(&self, coordinate: i32) -> f32 {
        coordinate as f32 * self.levels().frequency
    }
}

type TerrainDataFactory = Box<dyn Fn() -> TerrainData + Send + Sync>;

pub struct TerrainGenerator<N> {
    levels: TerrainLevels,
    noise_generator: N,
    terrain_data_pool: ObjectPool<TerrainData, TerrainDataFactory>,
}

impl<N: NoiseGenerator> TerrainGenerator<N> {
    pub fn new(levels: TerrainLevels, noise_generator: N, pool_capacity: i32) -> Self {
        let factory: TerrainDataFactory = Box::new(move || TerrainData::new(levels));
        Self {
            levels,
            noise_generator,
            terrain_data_pool: ObjectPool::new(pool_capacity, factory),
        }
    }

    pub const fn noise_generator(&self) -> &N {
        &self.noise_generator
    }

    pub fn restore(&self, terrain_data: TerrainData) {
        self.terrain_data_pool.restore(terrain_data);
    }

    pub fn generate(&self, seed: i32, chunk_x: i32, chunk_z: i32) -> TerrainData {
        let mut terrain_data = self.terrain_data_pool.take();
        self.noise_generator
            .generate(seed, chunk_x, chunk_z, &mut |noise_data| {
                terrain_data.accept(noise_data)
            });
        terrain_data
    }

    pub fn height(&self, seed: i32, x: i32, z: i32) -> i32 {
        let height_noise = self.noise_generator.height_noise(seed, x, z);
        self.levels.height(self.levels.scaled_height(height_noise))
    }
}

#[cfg(test)]
mod tests {
    use super::super::{
        terraforged_engine::TerrainRegistry,
        terraforged_terrain_data::{NoiseSample, TerrainData},
    };
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicI32, Ordering},
    };

    #[test]
    fn object_pool_matches_terraforged_source() {
        let next = Arc::new(AtomicI32::new(0));
        let counter = Arc::clone(&next);
        let pool = ObjectPool::new(2, move || counter.fetch_add(1, Ordering::SeqCst));
        assert_eq!(next.load(Ordering::SeqCst), 2);
        assert_eq!(pool.take(), 2);
        assert_eq!(pool.take(), 3);
        assert_eq!(pool.take(), 4);
        pool.restore(10);
        pool.restore(11);
        pool.restore(12);
        assert_eq!(pool.take(), 5);
        assert_eq!(pool.take(), 6);
    }

    struct TestNoise {
        levels: TerrainLevels,
    }

    impl NoiseGenerator for TestNoise {
        fn levels(&self) -> NoiseLevels {
            self.levels.noise_levels
        }

        fn terrain_levels(&self) -> TerrainLevels {
            self.levels
        }

        fn height_noise(&self, seed: i32, x: i32, z: i32) -> f32 {
            (seed.wrapping_add(x).wrapping_add(z) & 255) as f32 / 255.0
        }

        fn generate(
            &self,
            seed: i32,
            chunk_x: i32,
            chunk_z: i32,
            consumer: &mut dyn FnMut(&NoiseData),
        ) {
            let terrains = TerrainRegistry::new();
            let terrain = terrains.get_by_id(8);
            let mut data = NoiseData::new(&terrains);
            for z in NoiseData::MIN..NoiseData::MAX {
                for x in NoiseData::MIN..NoiseData::MAX {
                    let world_x = chunk_x.wrapping_mul(16).wrapping_add(x);
                    let world_z = chunk_z.wrapping_mul(16).wrapping_add(z);
                    data.set_noise(
                        x,
                        z,
                        &NoiseSample {
                            height_noise: self.height_noise(seed, world_x, world_z),
                            base_noise: 0.25,
                            terrain_type: terrain.clone(),
                            ..NoiseSample::new(&terrains)
                        },
                    );
                }
            }
            consumer(&data);
        }
    }

    #[test]
    fn terrain_generator_matches_terraforged_source_contract() {
        let levels = TerrainLevels::default();
        let generator = TerrainGenerator::new(levels, TestNoise { levels }, 1);
        assert_eq!(generator.height(7, 10, -3), 26);
        assert_eq!(
            generator.noise_generator().noise_coordinate(16).to_bits(),
            0x40eda076
        );
        let terrain: TerrainData = generator.generate(7, 2, -1);
        assert_eq!(terrain.height_at(0, 0), 43);
        assert_eq!(terrain.terrain().get(0, 0).unwrap().name(), "flats");
        generator.restore(terrain);
    }
}
