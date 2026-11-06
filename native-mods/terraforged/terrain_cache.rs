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

// Adapted from TerraForged Reimagined (https://github.com/Romoslayer/TerraForged-Reimagined, a24d450):
// `src/main/java/com/terraforged/mod/worldgen/terrain/TerrainCache.java` (6dd607eb's class plus
// `getErodedHeightNoise`). MIT License, Copyright (c) 2020 TerraForged, Copyright (c) 2026 Romoslayer; see
// `LICENSE-TERRAFORGED-REIMAGINED`.

use super::{
    terraforged_concurrent::{CacheEntry, ThreadPool},
    terraforged_eroded_noise_generator::ErodedNoiseGenerator,
    terraforged_terrain_data::{FloatMap, NoiseSample, TerrainData},
    terraforged_terrain_generator::{NoiseGenerator, TerrainGenerator, available_processors},
    terraforged_terrain_levels::TerrainLevels,
};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, MutexGuard, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard},
};

/// Java's `CacheKey`: the runtime seed and the chunk position. A hit needs all three.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct CacheKey {
    seed: i32,
    x: i32,
    z: i32,
}

/// One generated chunk's `TerrainData`, given back to the generator's pool (`generator.restore`) once nothing holds
/// it any more. Java restores it in `drop`, even while another reader may still hold it; its pool never hands a
/// restored value out again (`ObjectPool.take` always builds a new one), so restoring later changes nothing.
///
/// Java's stages share one mutable `TerrainData`: `fillFromNoise`'s `buildStructureTerrain` writes the structure
/// heights into it and the later stages of the chunk read them. The port keeps it behind an `RwLock` (a write
/// serializes with every reader) and also keeps the generated heights (`generated_height`), so the terrain stage can
/// start from them again ([`TerrainDataHandle::write_generated`]).
struct PooledTerrain<N: NoiseGenerator> {
    data: Option<RwLock<TerrainData>>,
    generated_height: FloatMap,
    generator: Arc<TerrainGenerator<N>>,
}

impl<N: NoiseGenerator> PooledTerrain<N> {
    fn new(data: TerrainData, generator: Arc<TerrainGenerator<N>>) -> Self {
        Self {
            generated_height: data.height().clone(),
            data: Some(RwLock::new(data)),
            generator,
        }
    }

    fn lock(&self) -> &RwLock<TerrainData> {
        self.data
            .as_ref()
            .expect("the data stays until the last handle is dropped")
    }
}

impl<N: NoiseGenerator> Drop for PooledTerrain<N> {
    fn drop(&mut self) {
        if let Some(data) = self.data.take() {
            self.generator
                .restore(data.into_inner().unwrap_or_else(PoisonError::into_inner));
        }
    }
}

/// A chunk's generated `TerrainData` (what Java's `join()` returns), shared by every reader of the same entry.
pub struct TerrainDataHandle<N: NoiseGenerator>(Arc<PooledTerrain<N>>);

impl<N: NoiseGenerator> Clone for TerrainDataHandle<N> {
    fn clone(&self) -> Self {
        Self(Arc::clone(&self.0))
    }
}

impl<N: NoiseGenerator> TerrainDataHandle<N> {
    /// Whether both handles hold the same generated data (Java's `==` on the `TerrainData`).
    pub fn ptr_eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl<N: NoiseGenerator> TerrainDataHandle<N> {
    /// Reads the shared data (Java's plain field reads), waiting for a writer. A panic under a write guard does not
    /// poison it: the next terrain stage starts again from the generated heights ([`Self::write_generated`]). Don't
    /// hold a guard of an entry while the same thread runs a terrain stage on it: the stage's write would wait for it
    /// forever (std's `RwLock` is not reentrant).
    pub fn read(&self) -> RwLockReadGuard<'_, TerrainData> {
        self.0.lock().read().unwrap_or_else(PoisonError::into_inner)
    }

    /// Writes the shared data in place (Java's `terrainData.getHeight().set(..)`); every handle of the entry sees it.
    pub fn write(&self) -> RwLockWriteGuard<'_, TerrainData> {
        self.0
            .lock()
            .write()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// [`Self::write`] with the heights first set back to what the generation produced (the rest of the data is never
    /// written). The terrain stage starts from here, so running it again for the same entry (a chunk generated again
    /// before `drop`) gives the first run's blocks and heights. Java's second run would read the heights its first
    /// `buildStructureTerrain` left, so its output would depend on whether and when the chunk was generated again;
    /// the port does not reproduce that.
    pub fn write_generated(&self) -> RwLockWriteGuard<'_, TerrainData> {
        let mut data = self.write();
        data.restore_height(&self.0.generated_height);
        data
    }
}

/// A chunk's generation task (Java's `CompletableFuture<TerrainData>`): a [`CacheEntry::compute_async`] entry.
pub struct TerrainTask<N: NoiseGenerator>(Arc<CacheEntry<'static, Arc<PooledTerrain<N>>>>);

impl<N: NoiseGenerator> Clone for TerrainTask<N> {
    fn clone(&self) -> Self {
        Self(Arc::clone(&self.0))
    }
}

impl<N: NoiseGenerator> TerrainTask<N> {
    /// `join()`: waits for the task and returns its data; panics, every time, if the generation panicked (Java's
    /// `CompletionException`).
    pub fn join(&self) -> TerrainDataHandle<N> {
        TerrainDataHandle(Arc::clone(self.0.get()))
    }

    /// `isDone()`: whether the task has ended, by returning or by panicking; it does not wait.
    pub fn is_done(&self) -> bool {
        self.0.is_done()
    }
}

/// TerraForged Reimagined 26.3's `worldgen.terrain.TerrainCache` (a24d450; 6dd607eb's plus `getErodedHeightNoise`;
/// the 1.18.2-0.3.1 jar's has no seed parameters and keys by `ChunkPos` alone): one generation task per runtime seed
/// and chunk, started on the cache's [`ThreadPool`] by the first `get_async`/`get_now`/`hint` and shared by every later
/// caller until [`Self::remove`] (Java's `drop`, called once the chunk is decorated) takes it out. As in Java, there
/// is no capacity, expiry or eviction: an entry stays until it is removed, so the map's size is bounded only by its
/// callers. Seeds are part of the key, so a hit never returns another seed's data, and a cached chunk is
/// [`TerrainGenerator::generate`]'s for the same seed and chunk.
///
/// The map is a `Mutex<HashMap>` (Java's `ConcurrentHashMap.computeIfAbsent`): the lock covers only the lookup and the
/// task's submission, never the generation, and nothing else is locked while it is held, so it never nests with the
/// eroded-region, cell or river caches the generation takes on the pool's threads. Engine's `Cache` and the mod's
/// `LossyCache` are not used: both evict (and the lossy one computes under its lock), which would drop tasks Java
/// keeps. Java's pooled keys and values (`keyPool`, `valuePool`, `localKey`) are not ported; keys are plain values.
pub struct TerrainCache<N: NoiseGenerator> {
    generator: Arc<TerrainGenerator<N>>,
    pool: ThreadPool,
    cache: Mutex<HashMap<CacheKey, TerrainTask<N>>>,
}

/// Minecraft's `Util.backgroundExecutor()` size (`Util.maxAllowedExecutorThreads`): `Mth.clamp(availableProcessors()
/// - 1, 1, 255)` (without a `max.bg.threads` property).
pub fn background_threads() -> i32 {
    available_processors().wrapping_sub(1).clamp(1, 255)
}

impl<N: NoiseGenerator + 'static> TerrainCache<N> {
    /// `new TerrainCache(levels, noiseGenerator)`: a [`TerrainGenerator`] over `noise_generator`, and generation tasks
    /// on [`background_threads`] task threads (Java's `ThreadPool.EXECUTOR`, Minecraft's background executor; the
    /// port's [`ThreadPool::create`] makes half its size in threads).
    pub fn new(levels: TerrainLevels, noise_generator: N) -> Self {
        Self::with_pool(
            levels,
            noise_generator,
            ThreadPool::create(background_threads().wrapping_mul(2)),
        )
    }

    /// [`Self::new`] with generation tasks on `pool`. On [`ThreadPool::NONE`] a task runs on the thread of its first
    /// `join` (so `get_if_ready` generates a requested chunk on its caller), as [`CacheEntry::compute_async`] does.
    pub fn with_pool(levels: TerrainLevels, noise_generator: N, pool: ThreadPool) -> Self {
        Self {
            generator: Arc::new(TerrainGenerator::new(levels, noise_generator)),
            pool,
            cache: Mutex::new(HashMap::with_capacity(512)),
        }
    }

    fn entries(&self) -> MutexGuard<'_, HashMap<CacheKey, TerrainTask<N>>> {
        self.cache.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// `drop(seed, pos)`: removes the chunk's entry and waits for its task (panicking if it panicked, as Java's
    /// `join` throws, the entry already removed); its data goes back to the pool once its last handle is dropped. No
    /// entry: nothing happens.
    pub fn remove(&self, seed: i32, x: i32, z: i32) {
        let task = self.entries().remove(&CacheKey { seed, x, z });
        if let Some(task) = task {
            drop(task.join());
        }
    }

    /// `hint(seed, pos)`: [`Self::get_async`], starting the chunk's generation if no entry has it.
    pub fn hint(&self, seed: i32, x: i32, z: i32) {
        self.get_async(seed, x, z);
    }

    /// `getHeight(seed, x, z)`: [`TerrainGenerator::height`], never cached.
    pub fn height(&self, seed: i32, x: i32, z: i32) -> i32 {
        self.generator.height(seed, x, z)
    }

    /// `getNow(seed, pos)`: [`Self::get_async`], then waits for it.
    pub fn get_now(&self, seed: i32, x: i32, z: i32) -> TerrainDataHandle<N> {
        self.get_async(seed, x, z).join()
    }

    /// `getIfReady(seed, pos)`: the chunk's data if its entry exists and its task has ended (panicking if the task
    /// panicked), else `None`. It never starts a task.
    pub fn get_if_ready(&self, seed: i32, x: i32, z: i32) -> Option<TerrainDataHandle<N>> {
        let task = self.entries().get(&CacheKey { seed, x, z }).cloned()?;
        task.is_done().then(|| task.join())
    }

    /// `getAsync(seed, pos)`: the chunk's task, submitting `generator.generate(seed, x, z)` to the pool if no entry has
    /// it yet (`computeIfAbsent`).
    pub fn get_async(&self, seed: i32, x: i32, z: i32) -> TerrainTask<N> {
        self.entries()
            .entry(CacheKey { seed, x, z })
            .or_insert_with(|| {
                let generator = Arc::clone(&self.generator);
                TerrainTask(Arc::new(CacheEntry::compute_async(
                    move || {
                        Arc::new(PooledTerrain::new(
                            generator.generate(seed, x, z),
                            Arc::clone(&generator),
                        ))
                    },
                    &self.pool,
                )))
            })
            .clone()
    }

    /// The number of entries (not in Java; for tests and diagnostics).
    pub fn len(&self) -> usize {
        self.entries().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// The queries Java's `TerrainCache` passes to its `INoiseGenerator`, for the eroded generator Reimagined builds it
/// on (the port's [`NoiseGenerator`] trait has no `erodedHeightNoise`/`getNoiseSample`/`sample`). None is cached.
impl TerrainCache<ErodedNoiseGenerator> {
    /// The eroded generator the cache builds its chunks from (Reimagined's `TerrainCache.generator`).
    pub fn noise_generator(&self) -> &ErodedNoiseGenerator {
        self.generator.noise_generator()
    }

    /// `getErodedHeightNoise(seed, x, z)`: [`ErodedNoiseGenerator::eroded_height_noise`].
    pub fn eroded_height_noise(&self, seed: i32, x: i32, z: i32) -> Result<f32, &'static str> {
        self.generator
            .noise_generator()
            .eroded_height_noise(seed, x, z)
    }

    /// `getSample(seed, x, z)`: [`ErodedNoiseGenerator::noise_sample`].
    pub fn noise_sample(&self, seed: i32, x: i32, z: i32) -> Result<NoiseSample, &'static str> {
        self.generator.noise_generator().noise_sample(seed, x, z)
    }

    /// `sample(seed, x, z, sample)`: [`ErodedNoiseGenerator::sample`].
    pub fn sample(
        &self,
        seed: i32,
        x: i32,
        z: i32,
        sample: &mut NoiseSample,
    ) -> Result<(), &'static str> {
        self.generator.noise_generator().sample(seed, x, z, sample)
    }
}

#[cfg(test)]
mod tests {
    use super::super::{
        terraforged::Module,
        terraforged_engine::TerrainRegistry,
        terraforged_erosion::{ErosionFilterSettings, HeightmapSmoothingSettings},
        terraforged_noise_generator::tests::{Fold, generator, generator_with},
        terraforged_terrain_blender::TerrainNoise,
        terraforged_terrain_data::NoiseData,
        terraforged_terrain_levels::NoiseLevels,
    };
    use super::*;
    use std::{
        fmt::Write,
        panic::{AssertUnwindSafe, catch_unwind},
        sync::{
            Condvar,
            atomic::{AtomicBool, Ordering},
        },
        thread,
    };

    const _: () = {
        const fn send_sync<T: Send + Sync>() {}
        send_sync::<TerrainCache<ErodedNoiseGenerator>>();
        send_sync::<TerrainTask<ErodedNoiseGenerator>>();
        send_sync::<TerrainDataHandle<ErodedNoiseGenerator>>();
    };

    /// Records every `generate` call; a chunk's data holds its call number as the river noise of cell (0, 0). It can
    /// wait behind a gate, or panic.
    struct Recorder {
        levels: TerrainLevels,
        terrains: TerrainRegistry,
        calls: Arc<Mutex<Vec<(i32, i32, i32)>>>,
        gate: Arc<(Mutex<bool>, Condvar)>,
        fail: AtomicBool,
    }

    impl Recorder {
        fn new() -> Self {
            Self {
                levels: TerrainLevels::default(),
                terrains: TerrainRegistry::new(),
                calls: Arc::default(),
                gate: Arc::new((Mutex::new(true), Condvar::new())),
                fail: AtomicBool::new(false),
            }
        }
    }

    impl NoiseGenerator for Recorder {
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
            let (lock, ready) = &*self.gate;
            let mut open = lock.lock().unwrap();
            while !*open {
                open = ready.wait(open).unwrap();
            }
            drop(open);
            assert!(!self.fail.load(Ordering::SeqCst), "generation failed");
            let serial = {
                let mut calls = self.calls.lock().unwrap();
                calls.push((seed, chunk_x, chunk_z));
                calls.len() - 1
            };
            let mut data = NoiseData::new(&self.terrains);
            let mut sample = NoiseSample::new(&self.terrains);
            sample.river_noise = serial as f32;
            data.set_noise(0, 0, &sample);
            consumer(&data);
        }
    }

    fn serial(data: Option<TerrainDataHandle<Recorder>>) -> String {
        data.map_or_else(
            || "none".to_owned(),
            |data| (data.read().river().get(0, 0) as i32).to_string(),
        )
    }

    /// `oracles/terrain_cache/out.txt` (`run.sh`): Reimagined's real `TerrainCache` over a recording generator stub
    /// on one FIFO task thread, interpreted and JIT-compiled alike (Java's restore counts are left out: the port's
    /// pool restores when the last handle is dropped).
    const JAVA: &str = "\
ready 1 0 0 none size 0
now 1 0 0 0
now 2 0 0 1
async 1 0 0 0
ready 1 0 0 0
ready 3 0 0 none size 2
drop 1 0 0 size 1
ready 1 0 0 none
now 1 0 0 2
drop 9 9 9 size 2
now -2147483648 -1 2147483647 3
now 1 0 1 4
now 1 1 0 5
now 5 1 2 7
ready 5 1 1 6
drop 2 0 0 size 6
call 0 1 0 0
call 1 2 0 0
call 2 1 0 0
call 3 -2147483648 -1 2147483647
call 4 1 0 1
call 5 1 1 0
call 6 5 1 1
call 7 5 1 2
";

    #[test]
    fn terrain_cache_trace_matches_reimagined() {
        let noise = Recorder::new();
        let calls = Arc::clone(&noise.calls);
        // One task thread, taking tasks in submission order (the oracle's single-thread executor).
        let cache = TerrainCache::with_pool(noise.levels, noise, ThreadPool::create(2));
        let (min, max) = (i32::MIN, i32::MAX);
        let mut out = String::new();
        let o = &mut out;
        writeln!(
            o,
            "ready 1 0 0 {} size {}",
            serial(cache.get_if_ready(1, 0, 0)),
            cache.len()
        )
        .unwrap();
        cache.hint(1, 0, 0);
        writeln!(o, "now 1 0 0 {}", serial(Some(cache.get_now(1, 0, 0)))).unwrap();
        writeln!(o, "now 2 0 0 {}", serial(Some(cache.get_now(2, 0, 0)))).unwrap();
        writeln!(
            o,
            "async 1 0 0 {}",
            serial(Some(cache.get_async(1, 0, 0).join()))
        )
        .unwrap();
        writeln!(o, "ready 1 0 0 {}", serial(cache.get_if_ready(1, 0, 0))).unwrap();
        writeln!(
            o,
            "ready 3 0 0 {} size {}",
            serial(cache.get_if_ready(3, 0, 0)),
            cache.len()
        )
        .unwrap();
        cache.remove(1, 0, 0);
        writeln!(o, "drop 1 0 0 size {}", cache.len()).unwrap();
        writeln!(o, "ready 1 0 0 {}", serial(cache.get_if_ready(1, 0, 0))).unwrap();
        writeln!(o, "now 1 0 0 {}", serial(Some(cache.get_now(1, 0, 0)))).unwrap();
        cache.remove(9, 9, 9);
        writeln!(o, "drop 9 9 9 size {}", cache.len()).unwrap();
        writeln!(
            o,
            "now {min} -1 {max} {}",
            serial(Some(cache.get_now(min, -1, max)))
        )
        .unwrap();
        writeln!(o, "now 1 0 1 {}", serial(Some(cache.get_now(1, 0, 1)))).unwrap();
        writeln!(o, "now 1 1 0 {}", serial(Some(cache.get_now(1, 1, 0)))).unwrap();
        cache.hint(5, 1, 1);
        cache.hint(5, 1, 2);
        writeln!(o, "now 5 1 2 {}", serial(Some(cache.get_now(5, 1, 2)))).unwrap();
        writeln!(o, "ready 5 1 1 {}", serial(cache.get_if_ready(5, 1, 1))).unwrap();
        cache.remove(2, 0, 0);
        writeln!(o, "drop 2 0 0 size {}", cache.len()).unwrap();
        for (index, (seed, x, z)) in calls.lock().unwrap().iter().enumerate() {
            writeln!(o, "call {index} {seed} {x} {z}").unwrap();
        }
        assert_eq!(out, JAVA);
    }

    #[test]
    fn terrain_cache_shares_running_tasks_and_reports_readiness() {
        let noise = Recorder::new();
        let calls = Arc::clone(&noise.calls);
        let gate = Arc::clone(&noise.gate);
        *gate.0.lock().unwrap() = false;
        let cache = TerrainCache::with_pool(noise.levels, noise, ThreadPool::create(4));
        let first = cache.get_async(7, 2, -1);
        let second = cache.get_async(7, 2, -1);
        assert!(!first.is_done());
        assert!(cache.get_if_ready(7, 2, -1).is_none());
        assert_eq!(cache.len(), 1);
        *gate.0.lock().unwrap() = true;
        gate.1.notify_all();
        let first_data = first.join();
        assert!(first_data.ptr_eq(&second.join()));
        assert!(second.is_done());
        assert!(
            cache
                .get_if_ready(7, 2, -1)
                .is_some_and(|data| data.ptr_eq(&first_data))
        );
        assert_eq!(cache.height(5, 10, -3), cache.generator.height(5, 10, -3));
        assert_eq!(calls.lock().unwrap().len(), 1);
    }

    /// Every handle of an entry shares one `TerrainData` (Java's one object): writes from several threads serialize
    /// and every handle sees them; `write_generated` sets the heights back to the generated ones; a removed entry's
    /// data stays with its handles while a new entry is generated afresh.
    #[test]
    fn handles_share_writes_and_write_generated_restores_heights() {
        let noise = Recorder::new();
        let cache = TerrainCache::with_pool(noise.levels, noise, ThreadPool::create(2));
        let handle = cache.get_now(7, 2, -1);
        let generated = handle.read().height_at(3, 4);
        thread::scope(|scope| {
            for _ in 0..4 {
                let handle = handle.clone();
                scope.spawn(move || {
                    for _ in 0..20 {
                        let mut data = handle.write();
                        let height = data.height_at(3, 4);
                        data.set_height_at(3, 4, height + 1);
                    }
                });
            }
        });
        let raised = generated + 80;
        assert_eq!(cache.get_now(7, 2, -1).read().height_at(3, 4), raised);
        assert_eq!(handle.read().height_at(4, 3), generated);
        {
            let entry = cache.get_now(7, 2, -1);
            let data = entry.write_generated();
            assert_eq!(data.height_at(3, 4), generated);
            assert_eq!(data.river().get(0, 0), 0.0);
        }
        assert_eq!(handle.read().height_at(3, 4), generated);
        handle.write().set_height_at(3, 4, raised);
        cache.remove(7, 2, -1);
        assert_eq!(handle.read().height_at(3, 4), raised);
        let fresh = cache.get_now(7, 2, -1);
        assert!(!fresh.ptr_eq(&handle));
        assert_eq!(serial(Some(fresh.clone())), "1");
        assert_eq!(fresh.read().height_at(3, 4), generated);
    }

    #[test]
    fn failed_generation_panics_every_reader_and_runs_once() {
        let noise = Recorder::new();
        noise.fail.store(true, Ordering::SeqCst);
        let cache = TerrainCache::with_pool(noise.levels, noise, ThreadPool::create(2));
        for _ in 0..2 {
            assert!(catch_unwind(AssertUnwindSafe(|| cache.get_now(3, 0, 0))).is_err());
        }
        assert!(catch_unwind(AssertUnwindSafe(|| cache.get_if_ready(3, 0, 0))).is_err());
        assert_eq!(cache.len(), 1);
        // Java's `drop` removes the entry, then its `join` throws.
        assert!(catch_unwind(AssertUnwindSafe(|| cache.remove(3, 0, 0))).is_err());
        assert!(cache.is_empty());
        cache
            .generator
            .noise_generator()
            .fail
            .store(false, Ordering::SeqCst);
        assert_eq!(serial(Some(cache.get_now(3, 0, 0))), "0");
    }

    /// Every field `TerrainData::accept` writes, bit for bit.
    fn fold(data: &TerrainData) -> u64 {
        let mut fold = Fold::new();
        for z in 0..16 {
            for x in 0..16 {
                fold.float(data.height().get(x, z));
                fold.float(data.base_height().get(x, z));
                fold.float(data.gradient().get(x, z));
                fold.float(data.river().get(x, z));
                fold.add(
                    data.terrain()
                        .get(x, z)
                        .map_or(u64::MAX, |terrain| terrain.id() as u64),
                );
            }
        }
        for value in [data.min(), data.max(), data.max_base()] {
            fold.add(value as u64);
        }
        fold.0
    }

    const SEEDS: [i32; 4] = [0, 42, i32::MIN, -17];
    const CHUNKS: [(i32, i32); 4] = [(0, 0), (-1, 3), (40, -25), (-300, -301)];

    #[test]
    fn cached_chunks_equal_uncached_across_seeds_and_threads() {
        let direct = TerrainGenerator::new(generator(0, 1).terrain_levels(), generator(0, 1));
        let expected: Vec<u64> = SEEDS
            .iter()
            .flat_map(|&seed| CHUNKS.map(|(x, z)| fold(&direct.generate(seed, x, z))))
            .collect();
        // The same chunk differs between seeds, so a hit keyed by position alone would show.
        assert!((0..CHUNKS.len()).all(|chunk| expected[chunk] != expected[CHUNKS.len() + chunk]));
        let cache = TerrainCache::with_pool(
            generator(0, 1).terrain_levels(),
            generator(0, 1),
            ThreadPool::create(6),
        );
        thread::scope(|scope| {
            for worker in 0..8 {
                let (cache, expected) = (&cache, &expected);
                scope.spawn(move || {
                    for step in 0..expected.len() {
                        let index = (step * 5 + worker * 3) % expected.len();
                        let (seed, (x, z)) =
                            (SEEDS[index / CHUNKS.len()], CHUNKS[index % CHUNKS.len()]);
                        if worker % 2 == 0 {
                            cache.hint(seed, x, z);
                        }
                        assert_eq!(
                            fold(&cache.get_now(seed, x, z).read()),
                            expected[index],
                            "{seed} {x} {z}"
                        );
                    }
                });
            }
        });
        assert_eq!(cache.len(), expected.len());
        // A removed chunk is generated again, equal to the first.
        cache.remove(42, -1, 3);
        assert_eq!(
            fold(&cache.get_now(42, -1, 3).read()),
            expected[CHUNKS.len() + 1]
        );
    }

    #[test]
    fn eroded_queries_are_the_eroded_generators() {
        let eroded = || {
            ErodedNoiseGenerator::new(
                generator(0, 1),
                ErosionFilterSettings::default(),
                HeightmapSmoothingSettings::default(),
            )
        };
        let direct = eroded();
        let levels = NoiseGenerator::terrain_levels(&direct);
        let cache = TerrainCache::new(levels, eroded());
        let terrains = Arc::clone(&direct.generator().continent().context().terrain);
        for (seed, x, z) in [(0, 5, 7), (i32::MIN, -70, 3), (99, 1000, -1000)] {
            let mut blender = direct.generator().blender_resource();
            let mut expected = NoiseSample::new(&terrains);
            direct
                .generator()
                .sample(seed, x, z, &mut expected, &mut blender)
                .unwrap();
            let mut fold_expected = Fold::new();
            fold_expected.sample(&expected);
            let mut fold_cached = Fold::new();
            fold_cached.sample(&cache.noise_sample(seed, x, z).unwrap());
            assert_eq!(fold_cached.0, fold_expected.0);
            // `sample` fills the caller's sample without resetting it first.
            let mut carried = NoiseSample::new(&terrains);
            carried.river_noise = 0.5;
            let mut carried_direct = carried.clone();
            cache.sample(seed, x, z, &mut carried).unwrap();
            direct.sample(seed, x, z, &mut carried_direct).unwrap();
            let (mut a, mut b) = (Fold::new(), Fold::new());
            a.sample(&carried);
            b.sample(&carried_direct);
            assert_eq!(a.0, b.0);
        }
        // One region (seed 3, chunks 0..8) for the eroded height and the chunk.
        assert_eq!(
            cache.eroded_height_noise(3, 5, 7).unwrap().to_bits(),
            direct.eroded_height_noise(3, 5, 7).unwrap().to_bits()
        );
        let uncached = TerrainGenerator::new(levels, eroded());
        assert_eq!(
            fold(&cache.get_now(3, 0, 0).read()),
            fold(&uncached.generate(3, 0, 0))
        );
    }

    /// A custom terrain noise has no seed-first form: the eroded queries pass the generator's `Err` on (Java throws),
    /// and the chunk task panics every reader, as Java's `join` throws.
    #[test]
    fn eroded_query_failures_are_errors() {
        let failing = ErodedNoiseGenerator::new(
            generator_with(0, |context| {
                vec![Arc::new(TerrainNoise::new(
                    context.terrain.get_by_name("hills").unwrap(),
                    1.0,
                    Module::custom(0.0, 1.0, |_, _| 0.25),
                ))]
            }),
            ErosionFilterSettings::default(),
            HeightmapSmoothingSettings::default(),
        );
        let levels = NoiseGenerator::terrain_levels(&failing);
        let terrains = Arc::clone(&failing.generator().continent().context().terrain);
        let cache = TerrainCache::with_pool(levels, failing, ThreadPool::create(2));
        assert!(cache.noise_sample(1, 2, 3).is_err());
        assert!(
            cache
                .sample(1, 2, 3, &mut NoiseSample::new(&terrains))
                .is_err()
        );
        assert!(cache.eroded_height_noise(1, 2, 3).is_err());
        assert!(catch_unwind(AssertUnwindSafe(|| cache.get_now(1, 0, 0))).is_err());
        assert!(cache.get_async(1, 0, 0).is_done());
    }
}
