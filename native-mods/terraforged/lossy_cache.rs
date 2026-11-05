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

use super::terraforged_terrain_generator::available_processors;
use std::{
    panic::RefUnwindSafe,
    sync::{Arc, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard},
};

/// The removal listener of a [`ConcurrentLossyCache`] (Java's `Consumer<T> removalListener`). It is
/// `RefUnwindSafe` so that a cache stays usable inside `Module::custom` closures.
pub type RemovalListener<T> = Box<dyn Fn(Arc<T>) + Send + Sync + RefUnwindSafe>;

/// One slot of a bucket: Java's `keys[index]` and `values[index]`, plus the seed the value was
/// computed with (see [`ConcurrentLossyCache`]). An empty slot has Java's initial key `Long.MIN_VALUE`.
struct Slot<T> {
    key: i64,
    seed: i32,
    value: Option<Arc<T>>,
}

impl<T> Slot<T> {
    fn hit(&self, seed: i32, key: i64) -> Option<Arc<T>> {
        match &self.value {
            Some(value) if self.key == key && self.seed == seed => Some(Arc::clone(value)),
            _ => None,
        }
    }
}

/// `LossyCache.Stamped`: a power-of-two array of slots under one lock. Java reads optimistically
/// (or under the read lock) and computes under the write lock; here hits take the read lock and a
/// miss takes the write lock, checks the slot again (Java's re-check after waiting for the write
/// lock) and computes under it.
struct Bucket<T> {
    mask: usize,
    slots: RwLock<Box<[Slot<T>]>>,
}

impl<T> Bucket<T> {
    /// `new Stamped(capacity, ...)`: `Mth.smallestEncompassingPowerOfTwo(capacity)` slots.
    fn new(capacity: i32) -> Self {
        let slots = smallest_encompassing_power_of_two(capacity);
        assert!(
            slots > 0,
            "ConcurrentLossyCache: a bucket of {capacity} slots (Java's arrays would be empty or negative)"
        );
        Self {
            mask: (slots - 1) as usize,
            slots: RwLock::new(
                (0..slots)
                    .map(|_| Slot {
                        key: i64::MIN,
                        seed: 0,
                        value: None,
                    })
                    .collect(),
            ),
        }
    }

    // A factory that panicked under the write lock changed no slot, so a poisoned bucket is still
    // consistent (Java's StampedLock has no poisoning).
    fn read(&self) -> RwLockReadGuard<'_, Box<[Slot<T>]>> {
        self.slots.read().unwrap_or_else(PoisonError::into_inner)
    }

    fn write(&self) -> RwLockWriteGuard<'_, Box<[Slot<T>]>> {
        self.slots.write().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The newer TerraForged source's bounded concurrent lossy cache (`LossyCache.Concurrent` over
/// `LossyCache.Stamped` buckets, `com.terraforged.mod.util.storage`, 6dd607eb): a fixed number of
/// slots, `long` keys, and a new key simply replaces whatever its slot held.
///
/// The structure is Java's: `smallestEncompassingPowerOfTwo(concurrency)` buckets, each with
/// `smallestEncompassingPowerOfTwo(NoiseUtil.floor((float) capacity / buckets))` slots; a key's
/// bucket is `(int) (key ^ (key >>> 16)) & 0x7fffffff` and its slot is fastutil's
/// `(int) HashCommon.mix(key)`, both masked. A miss computes `function(seed, key)` under the bucket's
/// write lock (as Java does: other callers of that bucket wait for it, so a function that uses the
/// same bucket never returns), stores it in the slot and then, after the lock is released, passes
/// the replaced value to the removal listener. With one seed and one thread, hits, misses, eviction
/// order and listener calls are Java's (checked by the oracle, see the README).
///
/// Three differences, all so that a value through the cache always equals the direct computation:
/// - A hit needs the slot's key **and seed** to match. Java's `LongCache` keys by `key` alone and
///   returns a value computed with another seed (the river-piece cache would hand one world's pieces
///   to another). A different seed is a miss that replaces the slot, which is Java's behaviour
///   whenever the seed doesn't change.
/// - Values are shared `Arc`s: an evicted value stays valid for everyone still holding it. The
///   listener receives the evicted `Arc`, so it cannot recycle a value that is still in use (only
///   `Arc::into_inner` could take it back, and only once no one holds it). TerraForged's listeners
///   return evicted values to an `ObjectPool`, and its `Stamped` calls the listener with the value it
///   read before locking even when it then returns another thread's value or the function throws, so
///   a value can be released twice or while still cached (harmless only because the source's
///   `ObjectPool.take` always builds a new value). The port never calls the listener for a value it
///   didn't evict.
/// - Sizes Java can't use panic at construction: no buckets or empty buckets (where Java's first
///   lookup throws), and bucket or slot counts past 2^30 (where Java's constructor throws).
pub struct ConcurrentLossyCache<T> {
    mask: usize,
    buckets: Box<[Bucket<T>]>,
    removal_listener: RemovalListener<T>,
}

impl<T> ConcurrentLossyCache<T> {
    /// `LossyCache.concurrent(capacity, constructor)`: `Environment.CORES` buckets
    /// (`Runtime.availableProcessors()`), no removal listener.
    pub fn concurrent(capacity: i32) -> Self {
        Self::new(capacity, available_processors())
    }

    /// `LossyCache.concurrent(capacity, constructor, removalListener)`: `Environment.CORES` buckets.
    pub fn concurrent_with_listener(capacity: i32, removal_listener: RemovalListener<T>) -> Self {
        Self::with_listener(capacity, available_processors(), removal_listener)
    }

    /// `LossyCache.concurrent(capacity, concurrency, constructor, t -> {})`.
    pub fn new(capacity: i32, concurrency: i32) -> Self {
        Self::with_listener(capacity, concurrency, Box::new(|_| {}))
    }

    /// `new LossyCache.Concurrent(capacity, concurrency, constructor, removalListener)`.
    pub fn with_listener(
        capacity: i32,
        concurrency: i32,
        removal_listener: RemovalListener<T>,
    ) -> Self {
        let buckets = smallest_encompassing_power_of_two(concurrency);
        assert!(
            buckets > 0,
            "ConcurrentLossyCache: {buckets} buckets for concurrency {concurrency} (Java's array would be empty or negative)"
        );
        let concurrency = buckets;
        // NoiseUtil.floor of a positive float is the truncating cast; a bucket below one slot panics.
        let capacity = (capacity as f32 / concurrency as f32) as i32;
        Self {
            mask: (concurrency - 1) as usize,
            buckets: (0..concurrency).map(|_| Bucket::new(capacity)).collect(),
            removal_listener,
        }
    }

    /// `computeIfAbsent(seed, key, function)`: the slot's value if it holds `key` computed with
    /// `seed`, else `function(seed, key)`, which replaces the slot (see the type's documentation).
    pub fn compute_if_absent(
        &self,
        seed: i32,
        key: i64,
        function: impl FnOnce(i32, i64) -> T,
    ) -> Arc<T> {
        let bucket = &self.buckets[spread(key) as usize & self.mask];
        let index = hash(key) as u32 as usize & bucket.mask;
        if let Some(value) = bucket.read()[index].hit(seed, key) {
            return value;
        }
        let mut slots = bucket.write();
        if let Some(value) = slots[index].hit(seed, key) {
            return value;
        }
        let value = Arc::new(function(seed, key));
        let slot = &mut slots[index];
        let removed = slot.value.replace(Arc::clone(&value));
        slot.key = key;
        slot.seed = seed;
        drop(slots);
        if let Some(removed) = removed {
            (self.removal_listener)(removed);
        }
        value
    }
}

/// Minecraft's `Mth.smallestEncompassingPowerOfTwo(int)`, Java `int` math: 0 for 0 and negatives,
/// `i32::MIN` past 2^30 and for `i32::MIN` itself (`MIN - 1` wraps).
fn smallest_encompassing_power_of_two(value: i32) -> i32 {
    let mut i = value.wrapping_sub(1);
    i |= i >> 1;
    i |= i >> 2;
    i |= i >> 4;
    i |= i >> 8;
    i |= i >> 16;
    i.wrapping_add(1)
}

/// `LossyCache.Concurrent.spread`: `(int) (h ^ (h >>> 16)) & HASH_BITS`.
fn spread(key: i64) -> i32 {
    ((key ^ ((key as u64) >> 16) as i64) as i32) & 0x7fff_ffff
}

/// `LossyCache.hash`: `(int) HashCommon.mix(key)` (fastutil).
fn hash(key: i64) -> i32 {
    let h = (key as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    let h = h ^ (h >> 32);
    (h ^ (h >> 16)) as i32
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Barrier, Mutex, OnceLock,
        atomic::{AtomicUsize, Ordering},
    };

    /// FNV-1a 64 over UTF-8 lines, each followed by `\n`, as the oracle folds them.
    struct Fnv {
        hash: u64,
        lines: usize,
    }

    impl Fnv {
        fn new() -> Self {
            Self {
                hash: 0xcbf2_9ce4_8422_2325,
                lines: 0,
            }
        }

        fn line(&mut self, line: &str) {
            for byte in line.bytes().chain([b'\n']) {
                self.hash = (self.hash ^ u64::from(byte)).wrapping_mul(0x100_0000_01b3);
            }
            self.lines += 1;
        }
    }

    /// The oracle's key pool for a cache of `total` slots.
    fn pool(total: i64) -> Vec<i64> {
        let mut keys: Vec<i64> = (0..2 * total).collect();
        for i in 0..total {
            keys.extend([i * 65536, -(i + 1), i << 32]);
        }
        keys.extend([i64::MIN, i64::MIN + 1, i64::MAX, 0x7fff_ffff, 0x8000_0000]);
        keys
    }

    const CONFIGS: [(i32, i32); 9] = [
        (12, 3),
        (1, 1),
        (2, 1),
        (33, 4),
        (17, 16),
        (2048, 6),
        (100, 7),
        (64, 64),
        (1024, 6),
    ];
    const SEEDS: [i32; 3] = [0, 12345, i32::MIN];

    /// `LossyCacheOracle` (`.local/terraforged/oracles/lossy_cache/out.txt`), per config: the bucket
    /// count, slots per bucket and the slot table's line count and hash; the trace's line count,
    /// factory calls and listener calls (the same for every seed); the trace hash per seed.
    const SIZES: [(usize, usize, usize, u64); 9] = [
        (4, 4, 86, 0xce57bd9e4088d041),
        (1, 1, 11, 0x884776e032037a05),
        (1, 2, 16, 0xec08c60bf638864a),
        (4, 8, 166, 0x4ae33b27733870ae),
        (16, 1, 86, 0x932928ab60ce7efa),
        (8, 256, 10246, 0x749dfd23f6759218),
        (8, 16, 646, 0x57e9c7c57978ca60),
        (64, 1, 326, 0xd6b298147ff83d17),
        (8, 128, 5126, 0x08a2fcbf5d431c87),
    ];
    const TRACES: [(usize, usize, usize); 9] = [
        (886, 281, 265),
        (105, 33, 32),
        (152, 47, 45),
        (1688, 529, 499),
        (860, 268, 252),
        (105903, 33467, 31456),
        (6680, 2114, 1986),
        (3400, 1082, 1018),
        (53038, 16774, 15764),
    ];
    const TRACE_HASHES: [[u64; 3]; 9] = [
        [0x70805cc3d0515914, 0x5507f4046d05f7fb, 0x584fdd01eaf6cb02],
        [0xf582bce972fb4d37, 0x3399a5de7fac0926, 0x109e889ae211061d],
        [0xc45fb7a2a7a73ca2, 0x5449836057c4111f, 0x6f2048b2db85910c],
        [0x3ccc089189a92293, 0xef75fba98d17ab46, 0x1533f803e79da535],
        [0x1a92444f5a11a51f, 0xdd3f8659594843dd, 0xb1e0d3cb3803ea0f],
        [0x053fa381a3f374c5, 0x012cf263315c1ed4, 0x1c9b8d57945cdfdb],
        [0xf294315300443e5c, 0xecb4b369618c0978, 0x7f97159ef32b3634],
        [0xdf3e6cdbb5b86cfb, 0x0f46e082b439c303, 0x248159488c6c500f],
        [0xd9ae0c46a9827247, 0xd79133c126887e1f, 0x59150d7da2474363],
    ];

    #[test]
    fn slots_and_single_thread_trace_match_current_source() {
        for (cfg, &(capacity, concurrency)) in CONFIGS.iter().enumerate() {
            let (buckets, slots, slot_lines, slot_hash) = SIZES[cfg];
            let (lines, misses, removals) = TRACES[cfg];
            let probe = ConcurrentLossyCache::<i32>::new(capacity, concurrency);
            assert_eq!(
                (probe.buckets.len(), probe.buckets[0].mask + 1),
                (buckets, slots)
            );
            let keys = pool((buckets * slots) as i64);
            let mut fnv = Fnv::new();
            fnv.line(&format!("c {capacity} {concurrency} {buckets} {slots}"));
            for &key in &keys {
                let bucket = spread(key) as usize & probe.mask;
                let slot = hash(key) as u32 as usize & probe.buckets[bucket].mask;
                fnv.line(&format!("s {key} {bucket} {slot}"));
            }
            assert_eq!(
                (fnv.lines, fnv.hash),
                (slot_lines, slot_hash),
                "slots {cfg}"
            );

            for (&seed, &expected) in SEEDS.iter().zip(&TRACE_HASHES[cfg]) {
                // The listener and the factory both write to the trace, as in the oracle.
                let fnv = Arc::new(Mutex::new(Fnv::new()));
                let removed = Arc::new(AtomicUsize::new(0));
                let cache = ConcurrentLossyCache::with_listener(capacity, concurrency, {
                    let (fnv, removed) = (Arc::clone(&fnv), Arc::clone(&removed));
                    Box::new(move |id: Arc<usize>| {
                        removed.fetch_add(1, Ordering::Relaxed);
                        fnv.lock().unwrap().line(&format!("r {id}"));
                    })
                });
                let mut counter = 0usize;
                let mut x = 0x5_deec_e66d_i64 ^ cfg as i64;
                for _ in 0..4 * keys.len() {
                    x = x
                        .wrapping_mul(6_364_136_223_846_793_005)
                        .wrapping_add(1_442_695_040_888_963_407);
                    let key = keys[((x as u64 >> 33) % keys.len() as u64) as usize];
                    let value = cache.compute_if_absent(seed, key, |s, k| {
                        counter += 1;
                        fnv.lock().unwrap().line(&format!("m {s} {k} {counter}"));
                        counter
                    });
                    fnv.lock().unwrap().line(&format!("a {key} {value}"));
                }
                let fnv = fnv.lock().unwrap();
                assert_eq!(
                    (
                        fnv.lines,
                        counter,
                        removed.load(Ordering::Relaxed),
                        fnv.hash
                    ),
                    (lines, misses, removals, expected),
                    "trace {cfg} seed {seed}"
                );
            }
        }
    }

    #[test]
    fn caches_are_shareable() {
        fn shareable<T: Send + Sync + RefUnwindSafe>() {}
        shareable::<ConcurrentLossyCache<Vec<u64>>>();
    }

    #[test]
    fn sizes_follow_java_int_math() {
        let pow2 = smallest_encompassing_power_of_two;
        let cases = [(0, 0), (-5, 0), (1, 1), (3, 4), (1 << 30, 1 << 30)];
        for (value, expected) in cases {
            assert_eq!(pow2(value), expected, "{value}");
        }
        assert_eq!(pow2((1 << 30) + 1), i32::MIN);
        assert_eq!(pow2(i32::MAX), i32::MIN);
        assert_eq!(pow2(i32::MIN), i32::MIN, "MIN - 1 wraps to MAX");
        // Environment.CORES buckets; the oracle checks Java's three-argument factory takes CORES.
        let cores = ConcurrentLossyCache::<i32>::concurrent(2048);
        let direct = ConcurrentLossyCache::<i32>::new(2048, available_processors());
        assert_eq!(cores.buckets.len(), pow2(available_processors()) as usize);
        assert_eq!(cores.buckets[0].mask, direct.buckets[0].mask);
        // 100 / 8 = 12.5 floors to 12, which rounds up to 16 slots.
        assert_eq!(ConcurrentLossyCache::<i32>::new(100, 7).buckets[0].mask, 15);
    }

    #[test]
    #[should_panic(expected = "a bucket of 0 slots")]
    fn fewer_slots_than_buckets_panics() {
        ConcurrentLossyCache::<i32>::new(3, 4);
    }

    #[test]
    #[should_panic(expected = "0 buckets")]
    fn no_buckets_panics() {
        ConcurrentLossyCache::<i32>::new(16, 0);
    }

    #[test]
    fn hits_need_the_same_seed() {
        let removed = Arc::new(Mutex::new(Vec::new()));
        let cache = ConcurrentLossyCache::with_listener(1, 1, {
            let removed = Arc::clone(&removed);
            Box::new(move |value: Arc<i32>| removed.lock().unwrap().push(*value))
        });
        let first = cache.compute_if_absent(42, 0, |seed, _| seed);
        assert!(Arc::ptr_eq(
            &first,
            &cache.compute_if_absent(42, 0, |_, _| unreachable!())
        ));
        // Java would hit here and return 42; the port recomputes and replaces the slot.
        assert_eq!(*cache.compute_if_absent(43, 0, |seed, _| seed), 43);
        assert_eq!(*removed.lock().unwrap(), [42]);
        assert_eq!(*first, 42, "an evicted value stays valid for its holders");
    }

    #[test]
    fn listener_runs_after_the_lock_and_never_on_a_failed_compute() {
        static CACHE: OnceLock<ConcurrentLossyCache<i32>> = OnceLock::new();
        static REMOVED: AtomicUsize = AtomicUsize::new(0);
        let cache = CACHE.get_or_init(|| {
            ConcurrentLossyCache::with_listener(
                1,
                1,
                Box::new(|_| {
                    REMOVED.fetch_add(1, Ordering::SeqCst);
                    // The same (only) slot: this would deadlock if the bucket were still locked.
                    let cache = CACHE.get().unwrap();
                    assert_eq!(*cache.compute_if_absent(0, 1, |_, _| unreachable!()), 2);
                }),
            )
        });
        assert_eq!(*cache.compute_if_absent(0, 0, |_, _| 1), 1);
        let failed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            cache.compute_if_absent(0, 1, |_, _| panic!("x"))
        }));
        assert!(failed.is_err());
        assert_eq!(
            REMOVED.load(Ordering::SeqCst),
            0,
            "Java's Stamped would notify here"
        );
        assert_eq!(*cache.compute_if_absent(0, 0, |_, _| unreachable!()), 1);
        assert_eq!(*cache.compute_if_absent(0, 1, |_, _| 2), 2);
        assert_eq!(REMOVED.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn concurrent_hits_on_one_key_compute_once() {
        let cache = ConcurrentLossyCache::new(16, 4);
        let calls = AtomicUsize::new(0);
        let start = Barrier::new(8);
        std::thread::scope(|scope| {
            for _ in 0..8 {
                scope.spawn(|| {
                    start.wait();
                    let value = cache.compute_if_absent(7, 17, |_, _| {
                        calls.fetch_add(1, Ordering::SeqCst);
                        std::thread::yield_now();
                        1
                    });
                    assert_eq!(*value, 1);
                });
            }
        });
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    /// A value whose identity and contents depend only on `(seed, key)`, so any reuse shows.
    fn direct(seed: i32, key: i64) -> Vec<u64> {
        let mut h = (key as u64) ^ (u64::from(seed as u32) << 17);
        (0..16)
            .map(|_| {
                h = h.wrapping_mul(0x9e37_79b9_7f4a_7c15).rotate_left(29) ^ 0xa5a5;
                h
            })
            .collect()
    }

    #[test]
    fn cached_equals_uncached_under_contention() {
        // 2 buckets of 4 slots and 48 keys, three seeds: nearly every lookup collides with
        // another thread's key. Listener returns are counted; recycling is only possible for
        // values nobody holds any more (`Arc::into_inner`).
        let evicted = Arc::new(AtomicUsize::new(0));
        let reclaimable = Arc::new(AtomicUsize::new(0));
        let cache = ConcurrentLossyCache::with_listener(8, 2, {
            let (evicted, reclaimable) = (Arc::clone(&evicted), Arc::clone(&reclaimable));
            Box::new(move |value: Arc<Vec<u64>>| {
                evicted.fetch_add(1, Ordering::Relaxed);
                if Arc::into_inner(value).is_some() {
                    reclaimable.fetch_add(1, Ordering::Relaxed);
                }
            })
        });
        let computed = AtomicUsize::new(0);
        let start = Barrier::new(16);
        std::thread::scope(|scope| {
            for thread in 0..16u64 {
                let (cache, computed, start) = (&cache, &computed, &start);
                scope.spawn(move || {
                    start.wait();
                    let mut held = Vec::new();
                    let mut x = thread.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1;
                    for step in 0..20_000 {
                        x ^= x << 13;
                        x ^= x >> 7;
                        x ^= x << 17;
                        let key = (x % 48) as i64 * 65536 - 24;
                        let seed = [0, -1, i32::MIN][(x >> 40) as usize % 3];
                        let value = cache.compute_if_absent(seed, key, |s, k| {
                            computed.fetch_add(1, Ordering::Relaxed);
                            direct(s, k)
                        });
                        assert_eq!(*value, direct(seed, key));
                        if step % 997 == 0 {
                            held.push((seed, key, value));
                        }
                    }
                    // Values held across thousands of evictions are untouched.
                    for (seed, key, value) in held {
                        assert_eq!(*value, direct(seed, key));
                    }
                });
            }
        });
        let computed = computed.load(Ordering::Relaxed);
        // Every value computed went into a slot; all but the 8 still cached were evicted once.
        let still_cached = cache
            .buckets
            .iter()
            .map(|bucket| {
                bucket
                    .read()
                    .iter()
                    .filter(|slot| slot.value.is_some())
                    .count()
            })
            .sum::<usize>();
        assert_eq!(evicted.load(Ordering::Relaxed) + still_cached, computed);
        assert!(computed > 1000 && reclaimable.load(Ordering::Relaxed) > 0);
    }
}
