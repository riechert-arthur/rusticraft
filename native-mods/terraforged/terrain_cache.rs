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
    terraforged_concurrent::CacheEntry,
    terraforged_eroded_noise_generator::ErodedNoiseGenerator,
    terraforged_terrain_data::{NoiseSample, TerrainData},
    terraforged_terrain_generator::{NoiseGenerator, TerrainGenerator},
    terraforged_terrain_levels::TerrainLevels,
};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, RwLock, RwLockReadGuard, RwLockWriteGuard},
};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct CacheKey {
    seed: i32,
    x: i32,
    z: i32,
}

struct PooledTerrain<N: NoiseGenerator> {
    data: Option<RwLock<TerrainData>>,
    generator: Arc<TerrainGenerator<N>>,
}

impl<N: NoiseGenerator> Drop for PooledTerrain<N> {
    fn drop(&mut self) {
        if let Some(data) = self.data.take() {
            self.generator.restore(
                data.into_inner()
                    .unwrap_or_else(|poisoned| poisoned.into_inner()),
            );
        }
    }
}

pub struct TerrainDataHandle<N: NoiseGenerator>(Arc<PooledTerrain<N>>);

impl<N: NoiseGenerator> Clone for TerrainDataHandle<N> {
    fn clone(&self) -> Self {
        Self(Arc::clone(&self.0))
    }
}

impl<N: NoiseGenerator> TerrainDataHandle<N> {
    pub fn ptr_eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }

    pub fn read(&self) -> RwLockReadGuard<'_, TerrainData> {
        self.0
            .data
            .as_ref()
            .expect("pooled terrain data is present")
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn write(&self) -> RwLockWriteGuard<'_, TerrainData> {
        self.0
            .data
            .as_ref()
            .expect("pooled terrain data is present")
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

pub struct TerrainTask<N: NoiseGenerator>(Arc<CacheEntry<Arc<PooledTerrain<N>>>>);

impl<N: NoiseGenerator> Clone for TerrainTask<N> {
    fn clone(&self) -> Self {
        Self(Arc::clone(&self.0))
    }
}

impl<N: NoiseGenerator> TerrainTask<N> {
    fn new(create: impl FnOnce() -> Arc<PooledTerrain<N>> + Send + 'static) -> Self
    where
        N: 'static,
    {
        Self(Arc::new(CacheEntry::compute_async(create)))
    }

    pub fn join(&self) -> TerrainDataHandle<N> {
        TerrainDataHandle(Arc::clone(self.0.get()))
    }

    pub fn is_done(&self) -> bool {
        self.0.is_done()
    }
}

pub struct TerrainCache<N: NoiseGenerator> {
    generator: Arc<TerrainGenerator<N>>,
    entries: Mutex<HashMap<CacheKey, TerrainTask<N>>>,
}

impl<N: NoiseGenerator + 'static> TerrainCache<N> {
    pub fn new(levels: TerrainLevels, noise_generator: N) -> Self {
        let capacity = std::thread::available_parallelism()
            .map_or(1, usize::from)
            .min(i32::MAX as usize) as i32;
        Self::with_pool_capacity(levels, noise_generator, capacity)
    }

    pub fn with_pool_capacity(
        levels: TerrainLevels,
        noise_generator: N,
        pool_capacity: i32,
    ) -> Self {
        Self {
            generator: Arc::new(TerrainGenerator::new(
                levels,
                noise_generator,
                pool_capacity,
            )),
            entries: Mutex::new(HashMap::with_capacity(512)),
        }
    }

    pub fn remove(&self, seed: i32, x: i32, z: i32) {
        let key = CacheKey { seed, x, z };
        let task = self
            .entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&key);
        if let Some(task) = task {
            drop(task.join());
        }
    }

    pub fn hint(&self, seed: i32, x: i32, z: i32) {
        self.get_async(seed, x, z);
    }

    pub fn height(&self, seed: i32, x: i32, z: i32) -> i32 {
        self.generator.height(seed, x, z)
    }

    pub fn get_now(&self, seed: i32, x: i32, z: i32) -> TerrainDataHandle<N> {
        self.get_async(seed, x, z).join()
    }

    pub fn get_if_ready(&self, seed: i32, x: i32, z: i32) -> Option<TerrainDataHandle<N>> {
        let key = CacheKey { seed, x, z };
        let task = self
            .entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&key)
            .cloned()?;
        task.is_done().then(|| task.join())
    }

    pub fn get_async(&self, seed: i32, x: i32, z: i32) -> TerrainTask<N> {
        let key = CacheKey { seed, x, z };
        let mut entries = self
            .entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        entries
            .entry(key)
            .or_insert_with(|| {
                let generator = Arc::clone(&self.generator);
                TerrainTask::new(move || {
                    let data = generator.generate(seed, x, z);
                    Arc::new(PooledTerrain {
                        data: Some(RwLock::new(data)),
                        generator,
                    })
                })
            })
            .clone()
    }

    pub fn len(&self) -> usize {
        self.entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl TerrainCache<ErodedNoiseGenerator> {
    pub fn noise_generator(&self) -> &ErodedNoiseGenerator {
        self.generator.noise_generator()
    }

    pub fn eroded_height_noise(&self, seed: i32, x: i32, z: i32) -> f32 {
        self.generator
            .noise_generator()
            .eroded_height_noise(seed, x, z)
    }

    pub fn noise_sample(&self, seed: i32, x: i32, z: i32) -> NoiseSample {
        self.generator.noise_generator().noise_sample(seed, x, z)
    }

    pub fn sample<'a>(
        &self,
        seed: i32,
        x: i32,
        z: i32,
        sample: &'a mut NoiseSample,
    ) -> &'a mut NoiseSample {
        self.generator.noise_generator().sample(seed, x, z, sample)
    }
}

#[cfg(test)]
mod tests {
    use super::super::{terraforged_engine::TerrainRegistry, terraforged_terrain_data::NoiseData};
    use super::*;
    use std::sync::{
        Condvar,
        atomic::{AtomicUsize, Ordering},
    };

    struct TestNoise {
        levels: TerrainLevels,
        calls: Arc<AtomicUsize>,
        gate: Arc<(Mutex<bool>, Condvar)>,
    }

    impl NoiseGenerator for TestNoise {
        fn levels(&self) -> super::super::terraforged_terrain_levels::NoiseLevels {
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
            self.calls.fetch_add(1, Ordering::SeqCst);
            let (lock, ready) = &*self.gate;
            let mut open = lock.lock().unwrap();
            while !*open {
                open = ready.wait(open).unwrap();
            }
            drop(open);
            let terrain = TerrainRegistry::new().get_by_id(8);
            let mut data = NoiseData::new();
            for z in NoiseData::MIN..NoiseData::MAX {
                for x in NoiseData::MIN..NoiseData::MAX {
                    let world_x = chunk_x.wrapping_shl(4).wrapping_add(x);
                    let world_z = chunk_z.wrapping_shl(4).wrapping_add(z);
                    data.set_noise(
                        x,
                        z,
                        &NoiseSample {
                            height_noise: self.height_noise(seed, world_x, world_z),
                            base_noise: 0.25,
                            terrain_type: terrain.clone(),
                            ..NoiseSample::default()
                        },
                    );
                }
            }
            consumer(&data);
        }
    }

    #[test]
    fn terrain_cache_shares_tasks_reports_readiness_and_removes_entries() {
        let calls = Arc::new(AtomicUsize::new(0));
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let levels = TerrainLevels::default();
        let cache = TerrainCache::with_pool_capacity(
            levels,
            TestNoise {
                levels,
                calls: Arc::clone(&calls),
                gate: Arc::clone(&gate),
            },
            1,
        );
        let first = cache.get_async(7, 2, -1);
        let second = cache.get_async(7, 2, -1);
        assert!(cache.get_if_ready(7, 2, -1).is_none());
        assert_eq!(cache.len(), 1);
        {
            let (lock, ready) = &*gate;
            *lock.lock().unwrap() = true;
            ready.notify_all();
        }
        let first_data = first.join();
        let second_data = second.join();
        assert!(first_data.ptr_eq(&second_data));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(first_data.read().height_at(0, 0), 43);
        first_data.write().set_height_at(0, 0, 70);
        assert_eq!(second_data.read().height_at(0, 0), 70);
        assert!(
            cache
                .get_if_ready(7, 2, -1)
                .is_some_and(|data| data.ptr_eq(&first_data))
        );

        cache.remove(7, 2, -1);
        assert!(cache.is_empty());
        assert_eq!(cache.get_now(7, 2, -1).read().height_at(0, 0), 43);
        assert_eq!(first_data.read().height_at(0, 0), 70);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn shared_handles_serialize_updates_and_restore_pooled_data() {
        let calls = Arc::new(AtomicUsize::new(0));
        let levels = TerrainLevels::default();
        let cache = TerrainCache::with_pool_capacity(
            levels,
            TestNoise {
                levels,
                calls,
                gate: Arc::new((Mutex::new(true), Condvar::new())),
            },
            1,
        );
        let handle = cache.get_now(7, 2, -1);
        std::thread::scope(|scope| {
            for _ in 0..4 {
                let handle = handle.clone();
                scope.spawn(move || {
                    for _ in 0..20 {
                        let mut data = handle.write();
                        let height = data.height_at(0, 0);
                        data.set_height_at(0, 0, height + 1);
                    }
                });
            }
        });
        assert_eq!(handle.read().height_at(0, 0), 123);
        cache.remove(7, 2, -1);
        drop(handle);
        assert_eq!(cache.get_now(7, 2, -1).read().height_at(0, 0), 43);
    }

    #[test]
    fn seed_is_part_of_the_cache_key_and_height_stays_direct() {
        let calls = Arc::new(AtomicUsize::new(0));
        let gate = Arc::new((Mutex::new(true), Condvar::new()));
        let levels = TerrainLevels::default();
        let cache = TerrainCache::with_pool_capacity(
            levels,
            TestNoise {
                levels,
                calls: Arc::clone(&calls),
                gate,
            },
            1,
        );
        cache.hint(1, 0, 0);
        cache.hint(2, 0, 0);
        assert_eq!(cache.get_now(1, 0, 0).read().height_at(0, 0), 1);
        assert_eq!(cache.get_now(2, 0, 0).read().height_at(0, 0), 3);
        assert_eq!(cache.height(5, 10, -3), 22);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }
}
