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

use std::sync::{Arc, Mutex};

struct Slot<T> {
    key: i64,
    value: Option<Arc<T>>,
}

pub struct ConcurrentLossyCache<T> {
    buckets: Vec<Mutex<Vec<Slot<T>>>>,
}

impl<T> ConcurrentLossyCache<T> {
    pub fn new(capacity: usize, concurrency: usize) -> Self {
        assert!(capacity > 0 && concurrency > 0);
        let concurrency = concurrency.next_power_of_two();
        let bucket_capacity = (capacity as f32 / concurrency as f32) as usize;
        assert!(bucket_capacity > 0);
        let bucket_capacity = bucket_capacity.next_power_of_two();
        Self {
            buckets: (0..concurrency)
                .map(|_| {
                    Mutex::new(
                        (0..bucket_capacity)
                            .map(|_| Slot {
                                key: i64::MIN,
                                value: None,
                            })
                            .collect(),
                    )
                })
                .collect(),
        }
    }

    pub fn get_or_compute(
        &self,
        seed: i32,
        key: i64,
        factory: impl FnOnce(i32, i64) -> T,
    ) -> Arc<T> {
        self.get_or_compute_with_removal(seed, key, factory, |_| {})
    }

    pub fn get_or_compute_with_removal(
        &self,
        seed: i32,
        key: i64,
        factory: impl FnOnce(i32, i64) -> T,
        on_remove: impl FnOnce(Arc<T>),
    ) -> Arc<T> {
        let spread = (key as u64 ^ (key as u64 >> 16)) as u32 & 0x7fff_ffff;
        let bucket_index = spread as usize & (self.buckets.len() - 1);
        let mut bucket = self.buckets[bucket_index]
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let index = mix(key) as usize & (bucket.len() - 1);
        let slot = &mut bucket[index];
        if slot.key == key
            && let Some(value) = &slot.value
        {
            return Arc::clone(value);
        }
        let value = Arc::new(factory(seed, key));
        let removed = slot.value.replace(Arc::clone(&value));
        slot.key = key;
        drop(bucket);
        if let Some(removed) = removed {
            on_remove(removed);
        }
        value
    }
}

fn mix(key: i64) -> u64 {
    let hash = (key as u64).wrapping_mul(0x9e3779b97f4a7c15);
    let hash = hash ^ (hash >> 32);
    hash ^ (hash >> 16)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn eviction_trace_matches_current_java_cache() {
        let cache = ConcurrentLossyCache::new(12, 3);
        let mut removed = Vec::new();
        let mut counter = 0;
        let mut hash = 0xcbf29ce484222325u64;
        for pass in 0..3 {
            for index in -8..=8 {
                let key = if index == -8 { i64::MIN } else { index };
                let value = cache.get_or_compute_with_removal(
                    pass,
                    key,
                    |seed, _| {
                        counter += 1;
                        counter + seed * 100
                    },
                    |value| removed.push(*value),
                );
                hash = (hash ^ *value as u64).wrapping_mul(1099511628211);
            }
        }
        for value in &removed {
            hash = (hash ^ *value as u64).wrapping_mul(1099511628211);
        }
        assert_eq!((hash, counter, removed.len()), (0x4d256bd519d63ed5, 35, 23));
    }

    #[test]
    fn hits_ignore_seed_and_eviction_keeps_borrowers_alive() {
        let cache = ConcurrentLossyCache::new(1, 1);
        let first = cache.get_or_compute(42, 0, |seed, _| seed);
        let same = cache.get_or_compute(43, 0, |seed, _| seed);
        assert!(Arc::ptr_eq(&first, &same));
        assert_eq!(*same, 42);
        let next = cache.get_or_compute_with_removal(
            44,
            1,
            |seed, _| seed,
            |removed| {
                assert!(Arc::ptr_eq(&first, &removed));
                assert_eq!(
                    *cache.get_or_compute(0, 1, |_, _| panic!("cache should hit")),
                    44
                );
            },
        );
        assert_eq!(*first, 42);
        assert_eq!(*next, 44);
    }

    #[test]
    fn concurrent_hits_compute_once() {
        let cache = ConcurrentLossyCache::new(16, 4);
        let calls = AtomicUsize::new(0);
        std::thread::scope(|scope| {
            for seed in 0..8 {
                let cache = &cache;
                let calls = &calls;
                scope.spawn(move || {
                    let value = cache
                        .get_or_compute(seed, 17, |_, _| calls.fetch_add(1, Ordering::SeqCst) + 1);
                    assert_eq!(*value, 1);
                });
            }
        });
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
}
