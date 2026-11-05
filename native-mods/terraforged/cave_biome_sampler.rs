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
// `src/main/java/com/terraforged/mod/worldgen/biome/CaveBiomeSampler.java` with `util/storage/WeightMap.java`.
// TerraForged 6dd607eb's CaveBiomeSampler has the choice noise and the two pools but no coverage mask, no
// `getLayerBiome` and no empty-pool fallback; those are Reimagined's. MIT License, Copyright (c) 2020 TerraForged,
// Copyright (c) 2026 Romoslayer; see `LICENSE-TERRAFORGED-REIMAGINED`.

use super::{
    terraforged::{clamp, simplex},
    terraforged_cave_noise::CaveType,
    terraforged_terrain_blend_util::WeightMap,
};
use std::sync::Arc;

/// `CaveBiomeSampler.OFFSET`: added (Java `int` wrap) to the seed for the biome-choice noise.
pub const OFFSET: i32 = 124_897;
/// Reimagined's `MASK_OFFSET`: added (Java `int` wrap) to the seed for the coverage mask.
pub const MASK_OFFSET: i32 = 987_541;
/// Reimagined's `maskFrequency` (`1F / 1600F`).
pub const MASK_FREQUENCY: f32 = 1.0 / 1600.0;
/// Reimagined's `maskThreshold`: a column with a mask below it has no cave biome.
pub const MASK_THRESHOLD: f32 = 0.62;

/// TerraForged Reimagined's `CaveBiomeSampler`: equal-weight `WeightMap`s for `CaveType.GLOBAL` and
/// `CaveType.UNIQUE`, chosen by one simplex noise at `1 / scale`. Coordinates are BLOCK coordinates: Reimagined's
/// `Source.getLayerBiome(x, z)` takes quart coordinates and passes `x << 2`, `z << 2` (Java `int` wrap) with
/// the captured `(int)` world seed, so the layer that wires #159's quart cave closure must shift the same way.
pub struct CaveBiomeSampler<T> {
    scale: i32,
    frequency: f32,
    global: WeightMap<T>,
    unique: WeightMap<T>,
}

impl<T> CaveBiomeSampler<T> {
    /// `new CaveBiomeSampler(scale, biomeMapManager)` (Reimagined's `Source` uses 800) over pools the caller
    /// builds: `global` is `terraforged:cave` when the registry has it (else empty), `special` the registry's
    /// biomes in registry order that pass `BiomeUtil.isNonSurface` (`ModTags.NON_SURFACE`, or a path starting with
    /// `cave/` or ending with `_caves`). An empty `special` falls back to `global` (the same values), as
    /// Reimagined does; both empty means every lookup is `None` (Java's null). Reimagined's startup warning about
    /// incomplete cave biomes is not ported.
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

    /// The equal-weight pool `typeMap.get(cave_type)` holds (UNIQUE already fallen back to GLOBAL if it was empty).
    pub const fn pool(&self, cave_type: CaveType) -> &WeightMap<T> {
        match cave_type {
            CaveType::Global => &self.global,
            CaveType::Unique => &self.unique,
        }
    }

    /// `getUnderGroundBiome(seed, x, z, type)`: `None` where Java returns null (empty pool, NaN noise).
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

    /// Reimagined's `getLayerBiome(seed, x, z)`: the UNIQUE cave biome where the coarse mask reaches
    /// [`MASK_THRESHOLD`], else `None` (the column keeps its surface biome).
    pub fn get_layer_biome(&self, seed: i32, x: i32, z: i32) -> Option<&Arc<T>> {
        let mask = sample(seed.wrapping_add(MASK_OFFSET), x, z, MASK_FREQUENCY);
        if mask < MASK_THRESHOLD {
            None
        } else {
            self.get_underground_biome(seed, x, z, CaveType::Unique)
        }
    }
}

/// `CaveBiomeSampler.sample`: `NoiseUtil.clamp((1 + Noise.singleSimplex(x * f, z * f, seed)) * 0.5F, 0, 1)`.
pub fn sample(seed: i32, x: i32, z: i32, frequency: f32) -> f32 {
    let nx = x as f32 * frequency;
    let nz = z as f32 * frequency;
    clamp((1.0 + simplex(nx, nz, seed)) * 0.5, 0.0, 1.0)
}

fn equal_weights<T>(values: Vec<Arc<T>>) -> WeightMap<T> {
    let weights = vec![1.0; values.len()];
    WeightMap::new(values, &weights)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixtures_match_reimagined_java() {
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

    /// `out.txt` of `.local/terraforged/oracles/cave_biome_sampler/run.sh`: Reimagined a24d450's own
    /// `CaveBiomeSampler` (with its `WeightMap`, `MathUtil`, `Noise` and `NoiseUtil`) compiled from source over stub
    /// registries, interpreted and with `-Xcomp`. One line per scale and pool: frequency bits, sample count, layer
    /// hits and an FNV-1a digest of (choice bits, mask bits, layer, GLOBAL, UNIQUE) per seed and position.
    const REIMAGINED: &str = "\
P 800 0 983815946 4960 1851 10677fdaefc54da0
P 800 1 983815946 4960 1851 4e47c1b973a30df5
P 800 2 983815946 4960 0 578793cc615a5e10
P 800 3 983815946 4960 1851 fb9a5d8840db740e
P 800 4 983815946 4960 1851 50023107ba43d1fd
P 800 5 983815946 4960 1851 c52bfdb2a6d7a8d0
P 1 0 1065353216 4960 1851 7f480583f71f63d3
P 1 1 1065353216 4960 1851 073f73a6e525218b
P 1 2 1065353216 4960 0 0fe101730fed8e8a
P 1 3 1065353216 4960 1851 4b75380105251b11
P 1 4 1065353216 4960 1851 9f3891a1cb2df1ff
P 1 5 1065353216 4960 1851 2c49f90f7f453e9a
P 3 0 1051372203 4960 1851 2affbdaedd6e286c
P 3 1 1051372203 4960 1851 ae9a5619b658542f
P 3 2 1051372203 4960 0 1da287319153b592
P 3 3 1051372203 4960 1851 992dc6d759e4f2e7
P 3 4 1051372203 4960 1851 f4fbf6d1deab4d44
P 3 5 1051372203 4960 1851 162aa6173d43544a
P 1600 0 975427338 4960 1851 2acf775fd652fd5e
P 1600 1 975427338 4960 1851 3db40edd7075e968
P 1600 2 975427338 4960 0 38bba00c61562f4d
P 1600 3 975427338 4960 1851 756da5b1479ef88b
P 1600 4 975427338 4960 1851 71276118531c0b4f
P 1600 5 975427338 4960 1851 fd315d58a740dc51
P 0 0 2139095040 4960 1851 b70e6e320b58c252
P 0 1 2139095040 4960 1851 a09f1bb3333a0cfe
P 0 2 2139095040 4960 0 3b79b5eb978475af
P 0 3 2139095040 4960 1851 648e18dd09b082c8
P 0 4 2139095040 4960 1851 f1bff1ee0fbc2da1
P 0 5 2139095040 4960 1851 5ea4913e00014dc7
P -800 0 -1163667702 4960 1851 8312b02b3ea40539
P -800 1 -1163667702 4960 1851 976303a12e8c8d69
P -800 2 -1163667702 4960 0 a2483470fe33e288
P -800 3 -1163667702 4960 1851 6481ea3a02462d95
P -800 4 -1163667702 4960 1851 e2efec8787289acb
P -800 5 -1163667702 4960 1851 ff50fb658542e714
P -2147483648 0 -1342177280 4960 1851 13df8f75084c861b
P -2147483648 1 -1342177280 4960 1851 04c7e372653f9fcc
P -2147483648 2 -1342177280 4960 0 3b52977c726202fd
P -2147483648 3 -1342177280 4960 1851 57b83ee938b8defe
P -2147483648 4 -1342177280 4960 1851 7496a069e3ced8ef
P -2147483648 5 -1342177280 4960 1851 8ba4a00cdd30cd09
P 2147483647 0 805306368 4960 1851 a6c68adc240f8107
P 2147483647 1 805306368 4960 1851 9486e2597aa028bc
P 2147483647 2 805306368 4960 0 2127c36897c990bd
P 2147483647 3 805306368 4960 1851 2084e27dc96139c6
P 2147483647 4 805306368 4960 1851 d99847194993f2a1
P 2147483647 5 805306368 4960 1851 3f09416ee0b67cd9
T 238080";

    struct Lcg(i64);

    impl Lcg {
        fn next(&mut self) -> i32 {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            ((self.0 as u64) >> 32) as i32
        }
    }

    fn fold(hash: &mut u64, value: i32) {
        for byte in value.to_le_bytes() {
            *hash ^= u64::from(byte);
            *hash = hash.wrapping_mul(0x100_0000_01b3);
        }
    }

    #[test]
    fn sampler_matches_reimagined_java() {
        // The oracle's registries: special (cave) biome i has id i, `terraforged:cave` (GLOBAL) 1000; plain biomes
        // never reach the sampler. {has terraforged:cave, special count}.
        let pools = [
            (true, 3),
            (true, 0),
            (false, 0),
            (true, 7),
            (false, 5),
            (true, 1),
        ];
        let scales = [800, 1, 3, 1600, 0, -800, i32::MIN, i32::MAX];
        let mut random = Lcg(160);
        let mut seeds = vec![
            0,
            1,
            -1,
            123_456_789,
            -987_654_321,
            i32::MAX,
            i32::MIN,
            -OFFSET,
            i32::MAX - OFFSET + 1,
            -MASK_OFFSET,
        ];
        seeds.extend((0..6).map(|_| random.next()));
        let mut positions = vec![
            (0, 0),
            (1, -1),
            (1024, -768),
            (-2048, 4096),
            (777, -333),
            (i32::MAX, i32::MIN),
            (i32::MIN, i32::MAX),
            (i32::MAX, i32::MAX),
            (i32::MIN, i32::MIN),
            (-1, -1),
        ];
        for k in 0..300 {
            let (a, b) = (random.next(), random.next());
            positions.push(if k % 2 == 0 {
                ((a % 16384) << 2, (b % 16384) << 2)
            } else {
                (a, b)
            });
        }
        let id = |value: Option<&Arc<i32>>| value.map_or(-1, |value| **value);
        let mut lines = REIMAGINED.lines();
        let mut total = 0;
        for scale in scales {
            for (pool, &(has_cave, specials)) in pools.iter().enumerate() {
                let global = if has_cave {
                    vec![Arc::new(1000)]
                } else {
                    Vec::new()
                };
                let sampler =
                    CaveBiomeSampler::new(scale, global, (0..specials).map(Arc::new).collect());
                let (mut hash, mut hits, mut count) = (0xcbf2_9ce4_8422_2325_u64, 0, 0);
                for &seed in &seeds {
                    for &(x, z) in &positions {
                        let choice = sample(seed.wrapping_add(OFFSET), x, z, sampler.frequency());
                        fold(&mut hash, choice.to_bits() as i32);
                        let mask = sample(seed.wrapping_add(MASK_OFFSET), x, z, MASK_FREQUENCY);
                        fold(&mut hash, mask.to_bits() as i32);
                        let layer = id(sampler.get_layer_biome(seed, x, z));
                        hits += i32::from(layer != -1);
                        fold(&mut hash, layer);
                        fold(
                            &mut hash,
                            id(sampler.get_underground_biome(seed, x, z, CaveType::Global)),
                        );
                        fold(
                            &mut hash,
                            id(sampler.get_underground_biome(seed, x, z, CaveType::Unique)),
                        );
                        count += 1;
                    }
                }
                total += count;
                let line = format!(
                    "P {scale} {pool} {} {count} {hits} {hash:016x}",
                    sampler.frequency().to_bits() as i32
                );
                assert_eq!(
                    Some(line.as_str()),
                    lines.next(),
                    "scale {scale}, pool {pool}"
                );
            }
        }
        assert_eq!(Some(format!("T {total}").as_str()), lines.next());
        assert_eq!(None, lines.next());
    }
}
