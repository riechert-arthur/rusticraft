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
    terraforged::floor,
    terraforged_terrain_data::{NoiseData, TerrainData},
    terraforged_terrain_levels::{NoiseLevels, TerrainLevels},
};
use std::{collections::VecDeque, sync::Mutex};

/// The pool's counter and queue (Java's `AtomicInteger size` and `ConcurrentLinkedDeque pool`), under one lock.
struct PoolState<T> {
    size: i32,
    pool: VecDeque<T>,
}

/// TerraForged's `util.ObjectPool`, quirks included: `take` polls the queue but always returns a NEW value
/// from the factory (the polled one is dropped), and `restore` keeps a value only while its counter, which
/// counts up to `capacity + 1`, stays below that. `size` and `capacity` are Java `int`s.
pub struct ObjectPool<T, F> {
    capacity: i32,
    max_index: i32,
    factory: F,
    state: Mutex<PoolState<T>>,
}

impl<T, F: Fn() -> T> ObjectPool<T, F> {
    /// `new ObjectPool(capacity, factory)`: calls the factory `capacity` times (none when it is not positive).
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

    /// `new ObjectPool(factory)`: the capacity is `Environment.CORES` (`availableProcessors()`).
    pub fn with_cores(factory: F) -> Self {
        Self::new(available_processors(), factory)
    }

    /// `ObjectPool.forCacheSize`: capacity `NoiseUtil.floor(cacheSize * 1.75f)`.
    pub fn for_cache_size(cache_size: i32, factory: F) -> Self {
        Self::new(floor(cache_size as f32 * 1.75), factory)
    }

    pub fn take(&self) -> T {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if state.pool.pop_front().is_some() {
            state.size = state.size.wrapping_sub(1);
        }
        drop(state);
        (self.factory)()
    }

    pub fn restore(&self, value: T) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.size = if state.size < self.capacity {
            state.size.wrapping_add(1)
        } else {
            self.max_index
        };
        if state.size < self.max_index {
            state.pool.push_back(value);
        }
    }
}

/// Java's `Runtime.availableProcessors()` (at least 1).
fn available_processors() -> i32 {
    std::thread::available_parallelism().map_or(1, |n| i32::try_from(n.get()).unwrap_or(i32::MAX))
}

/// The part of TerraForged's `INoiseGenerator` that `TerrainGenerator` uses. As in the mod, the seed is
/// bound into the generator (Java's `with(seed, levels)`), so calls take only coordinates. The rest of the
/// interface (`with`, `getContinent`, `getNoiseSample`, `sample`, `find`) comes with the noise generator.
pub trait NoiseGenerator: Send + Sync {
    fn levels(&self) -> NoiseLevels;

    fn terrain_levels(&self) -> TerrainLevels;

    fn height_noise(&self, x: i32, z: i32) -> f32;

    /// Fills one chunk's `NoiseData` and hands it to `consumer` (Java's `Consumer<NoiseData>`).
    fn generate(&self, chunk_x: i32, chunk_z: i32, consumer: &mut dyn FnMut(&NoiseData));

    /// `getNoiseCoord`: `coord * getLevels().frequency` in `float` math.
    fn noise_coordinate(&self, coordinate: i32) -> f32 {
        coordinate as f32 * self.levels().frequency
    }
}

type TerrainDataFactory = Box<dyn Fn() -> TerrainData + Send + Sync>;

/// TerraForged's `worldgen.terrain.TerrainGenerator` (1.18.2-0.3.1): turns a noise generator's chunk noise
/// into `TerrainData`. Its pool (capacity `Environment.CORES`) never hands out restored data: every
/// `generate` fills a fresh `TerrainData`.
pub struct TerrainGenerator<N> {
    levels: TerrainLevels,
    noise_generator: N,
    terrain_data_pool: ObjectPool<TerrainData, TerrainDataFactory>,
}

impl<N: NoiseGenerator> TerrainGenerator<N> {
    pub fn new(levels: TerrainLevels, noise_generator: N) -> Self {
        let factory: TerrainDataFactory = Box::new(move || TerrainData::new(levels));
        Self {
            levels,
            noise_generator,
            terrain_data_pool: ObjectPool::with_cores(factory),
        }
    }

    pub const fn noise_generator(&self) -> &N {
        &self.noise_generator
    }

    pub fn restore(&self, terrain_data: TerrainData) {
        self.terrain_data_pool.restore(terrain_data);
    }

    pub fn generate(&self, chunk_x: i32, chunk_z: i32) -> TerrainData {
        let mut terrain_data = self.terrain_data_pool.take();
        self.noise_generator
            .generate(chunk_x, chunk_z, &mut |noise_data| {
                terrain_data.accept(noise_data)
            });
        terrain_data
    }

    /// `getHeight(x, z)`: `levels.getHeight(levels.getScaledHeight(noise.getHeightNoise(x, z)))`.
    pub fn height(&self, x: i32, z: i32) -> i32 {
        let height_noise = self.noise_generator.height_noise(x, z);
        self.levels.height(self.levels.scaled_height(height_noise))
    }
}

#[cfg(test)]
mod tests {
    use super::super::{
        terraforged_engine::TerrainRegistry, terraforged_terrain_data::NoiseSample,
    };
    use super::*;
    use std::{
        fmt::Write,
        sync::{
            Arc,
            atomic::{AtomicI32, Ordering},
        },
    };

    /// `Float.floatToIntBits`: every NaN prints as 7fc00000.
    fn h(v: f32) -> String {
        format!("{:08x}", if v.is_nan() { 0x7fc0_0000 } else { v.to_bits() })
    }

    fn fnv(text: &str) -> u64 {
        text.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
        })
    }

    const VALUES: [f32; 20] = [
        0.0,
        -0.0,
        1.0,
        -1.0,
        0.25,
        -0.25,
        0.5,
        0.999999,
        1.5,
        -480.0,
        62.5,
        1e-3,
        f32::from_bits(1),
        -f32::from_bits(1),
        3e9,
        -3e9,
        f32::MAX,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NAN,
    ];

    /// TerrainGeneratorOracle's integer hash (Java `int` math).
    fn hash(x: i32, z: i32, salt: i32) -> i32 {
        let mut r =
            x.wrapping_mul(0x9E37_79B1_u32 as i32) ^ z.wrapping_mul(0x85EB_CA6B_u32 as i32) ^ salt;
        r ^= ((r as u32) >> 15) as i32;
        r = r.wrapping_mul(0x2C1B_3C6D);
        r ^= ((r as u32) >> 12) as i32;
        r = r.wrapping_mul(0x297A_2D39);
        r ^= ((r as u32) >> 15) as i32;
        r
    }

    fn value(x: i32, z: i32, salt: i32, kind: i32) -> f32 {
        let r = hash(x, z, salt) as u32;
        if kind == 1 {
            VALUES[((r >> 1) % VALUES.len() as u32) as usize]
        } else {
            (r >> 8) as f32 * f32::from_bits(0x3380_0000)
        }
    }

    /// The oracle's stub `INoiseGenerator`; terrains come from the world registry the test checks against.
    struct Noise {
        levels: TerrainLevels,
        kind: i32,
        terrains: Arc<TerrainRegistry>,
        generated: AtomicI32,
    }

    impl NoiseGenerator for Noise {
        fn levels(&self) -> NoiseLevels {
            self.levels.noise_levels
        }

        fn terrain_levels(&self) -> TerrainLevels {
            self.levels
        }

        fn height_noise(&self, x: i32, z: i32) -> f32 {
            value(x, z, 1, self.kind)
        }

        fn generate(&self, chunk_x: i32, chunk_z: i32, consumer: &mut dyn FnMut(&NoiseData)) {
            self.generated.fetch_add(1, Ordering::SeqCst);
            let mut data = NoiseData::new(&self.terrains);
            let mut sample = NoiseSample::new(&self.terrains);
            for z in NoiseData::MIN..NoiseData::MAX {
                for x in NoiseData::MIN..NoiseData::MAX {
                    let wx = chunk_x.wrapping_shl(4).wrapping_add(x);
                    let wz = chunk_z.wrapping_shl(4).wrapping_add(z);
                    sample.reset(&self.terrains);
                    sample.height_noise = self.height_noise(wx, wz);
                    sample.base_noise = value(wx, wz, 2, self.kind);
                    sample.river_noise = if hash(wx, wz, 3) & 3 == 0 {
                        0.0
                    } else {
                        value(wx, wz, 4, self.kind)
                    };
                    sample.terrain_type = self.terrains.get_by_id(hash(wx, wz, 5) & 15);
                    data.set_noise(x, z, &sample);
                }
            }
            consumer(&data);
        }
    }

    fn pool_state(pool: &ObjectPool<i32, impl Fn() -> i32>) -> String {
        let state = pool.state.lock().unwrap();
        format!(
            "{} {} {} {:?}",
            pool.capacity, pool.max_index, state.size, state.pool
        )
    }

    #[test]
    fn terrain_generator_matches_terraforged_oracle() {
        let mut out = String::new();
        for capacity in [0, 1, 2, 3, -2] {
            let counter = Arc::new(AtomicI32::new(0));
            let calls = Arc::clone(&counter);
            let pool = ObjectPool::new(capacity, move || calls.fetch_add(1, Ordering::SeqCst));
            write!(
                out,
                "P {capacity} {} | {}",
                counter.load(Ordering::SeqCst),
                pool_state(&pool)
            )
            .unwrap();
            let mut restored = 100;
            for op in "ttrrrrtttrtrrrrrtt".chars() {
                if op == 't' {
                    write!(out, " | t {}", pool.take()).unwrap();
                } else {
                    pool.restore(restored);
                    restored += 1;
                    out.push_str(" | r");
                }
                write!(out, " {}", pool_state(&pool)).unwrap();
            }
            out.push('\n');
        }
        out.push('S');
        for size in [0, 1, 2, 3, 4, 7, -1, -2, -3, 1000, -1000, -4, -8] {
            write!(out, " {}", ObjectPool::for_cache_size(size, || 0).capacity).unwrap();
        }
        out.push('\n');

        // One registry per world: every terrain in the generated data is that registry's own.
        let terrains = Arc::new(TerrainRegistry::new());
        let levels = [
            TerrainLevels::default(),
            TerrainLevels::unconfigured(),
            TerrainLevels::new(true, 2.5, -32, 300, 100, 80, 20),
            TerrainLevels::new(true, 1.0, -64, 320, 128, 63, 23),
        ];
        let coords = [
            0,
            1,
            -1,
            15,
            16,
            -17,
            1000,
            -1000,
            30_000_000,
            i32::MAX,
            i32::MIN,
        ];
        let chunks = [
            (0, 0),
            (-1, 2),
            (134_217_727, -134_217_728),
            (i32::MAX, i32::MIN),
        ];
        for (l, &level) in levels.iter().enumerate() {
            for kind in 0..2 {
                let generator = TerrainGenerator::new(
                    level,
                    Noise {
                        levels: level,
                        kind,
                        terrains: Arc::clone(&terrains),
                        generated: AtomicI32::new(0),
                    },
                );
                let noise = generator.noise_generator();
                write!(out, "H {l} {kind} true").unwrap();
                for z in coords {
                    for x in coords {
                        write!(out, " {}", generator.height(x, z)).unwrap();
                    }
                }
                for c in coords {
                    write!(out, " {}", h(noise.noise_coordinate(c))).unwrap();
                }
                out.push('\n');
                for (cx, cz) in chunks {
                    let data = generator.generate(cx, cz);
                    write!(
                        out,
                        "G {l} {kind} {cx} {cz} {} false {} {} {}",
                        noise.generated.load(Ordering::SeqCst),
                        data.min(),
                        data.max(),
                        data.max_base()
                    )
                    .unwrap();
                    for z in 0..16 {
                        for x in 0..16 {
                            let terrain = data.terrain().get(x, z).unwrap();
                            let expected = terrains.get_by_id(terrain.id());
                            assert!(terrain.is_same(&expected));
                            write!(
                                out,
                                " {},{},{},{},{},{}",
                                h(data.height().get(x, z)),
                                h(data.base_height().get(x, z)),
                                h(data.gradient().get(x, z)),
                                h(data.river().get(x, z)),
                                terrain.id(),
                                data.height_at(x, z)
                            )
                            .unwrap();
                        }
                    }
                    out.push('\n');
                    generator.restore(data);
                }
            }
        }
        assert_eq!(out.lines().count(), 46);
        assert_eq!(
            out.lines().nth(5),
            Some("S 0 1 3 5 7 12 -2 -4 -6 1750 -1751 -8 -15")
        );
        assert_eq!(fnv(&out), 0xec71_67f2_41b9_b0a2);
    }

    /// Chunk generation shares one generator (and its pool) across worker threads, as the mod's chunk
    /// generator does: every thread gets the same data as a lone caller.
    #[test]
    fn terrain_generator_is_shareable_across_threads() {
        fn shareable<T: Send + Sync>() {}
        shareable::<TerrainGenerator<Noise>>();
        shareable::<ObjectPool<TerrainData, TerrainDataFactory>>();

        let terrains = Arc::new(TerrainRegistry::new());
        let levels = TerrainLevels::default();
        let generator = TerrainGenerator::new(
            levels,
            Noise {
                levels,
                kind: 0,
                terrains: Arc::clone(&terrains),
                generated: AtomicI32::new(0),
            },
        );
        let snapshot = |data: &TerrainData| {
            let mut out = format!("{} {} {}", data.min(), data.max(), data.max_base());
            for z in 0..16 {
                for x in 0..16 {
                    let terrain = data.terrain().get(x, z).unwrap();
                    write!(
                        out,
                        " {},{},{}",
                        h(data.height().get(x, z)),
                        h(data.gradient().get(x, z)),
                        terrain.id()
                    )
                    .unwrap();
                }
            }
            out
        };
        let chunks: Vec<(i32, i32)> = (-4..4).flat_map(|z| (-4..4).map(move |x| (x, z))).collect();
        let expected: Vec<String> = chunks
            .iter()
            .map(|&(x, z)| {
                let data = generator.generate(x, z);
                let text = snapshot(&data);
                generator.restore(data);
                text
            })
            .collect();
        std::thread::scope(|scope| {
            let workers: Vec<_> = (0..4)
                .map(|_| {
                    scope.spawn(|| {
                        chunks
                            .iter()
                            .map(|&(x, z)| {
                                let data = generator.generate(x, z);
                                let text = snapshot(&data);
                                generator.restore(data);
                                text
                            })
                            .collect::<Vec<_>>()
                    })
                })
                .collect();
            for worker in workers {
                assert_eq!(worker.join().unwrap(), expected);
            }
        });
        assert_eq!(
            generator.noise_generator().generated.load(Ordering::SeqCst),
            5 * 64
        );
        let state = generator.terrain_data_pool.state.lock().unwrap();
        assert!(state.pool.len() <= usize::try_from(available_processors()).unwrap());
    }
}
