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

// Adapted from TerraForged 6dd607eb (https://github.com/TerraForged/TerraForged/tree/6dd607ebfd41a3b7274e090b1f2cf565d53d1d44,
// MIT License, Copyright (c) 2021 TerraForged): `src/main/java/com/terraforged/mod/worldgen/biome/CaveBiomeSampler.java`
// with `util/storage/WeightMap.java`: the choice noise and two independent pools, an empty pool staying empty.

use super::{
    terraforged::{clamp, simplex},
    terraforged_cave_noise::CaveType,
    terraforged_terrain_blend_util::WeightMap,
};
use std::sync::Arc;

/// `CaveBiomeSampler.OFFSET`: added (Java `int` wrap) to the seed for the biome-choice noise.
pub const OFFSET: i32 = 124_897;

/// TerraForged's `CaveBiomeSampler`: equal-weight `WeightMap`s for `CaveType.GLOBAL` and `CaveType.UNIQUE`, chosen by
/// one simplex noise at `1 / scale`. Coordinates are BLOCK coordinates: `Source.getUnderGroundBiome(seed, x, z, type)`
/// adds the noise cave's seed to `Source`'s `(int)` world seed (Java `int` wrap) and passes the carver's block
/// position. TerraForged's chunk biome fill (`biome_chunk_fill.rs`) never reads it: only the noise-cave carver does.
pub struct CaveBiomeSampler<T> {
    scale: i32,
    frequency: f32,
    global: WeightMap<T>,
    unique: WeightMap<T>,
}

impl<T> CaveBiomeSampler<T> {
    /// TerraForged 6dd607eb's `new CaveBiomeSampler(scale, biomeMapManager)` (its `Source` uses 800) over pools the
    /// caller builds: `global` is `terraforged:cave` when the registry has it (else empty), `special` the UNIQUE pool
    /// (6dd607eb: every registry biome in registry order, its UNDERGROUND filter being commented out). The pools are
    /// independent: an empty one stays empty and every lookup in it is `None` (Java's `WeightMap.getValue` null).
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

    /// The equal-weight pool `typeMap.get(cave_type)` holds.
    pub const fn pool(&self, cave_type: CaveType) -> &WeightMap<T> {
        match cave_type {
            CaveType::Global => &self.global,
            CaveType::Unique => &self.unique,
        }
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

    /// The choice noise's bits at four fixtures (the same noise in TerraForged 6dd607eb and Reimagined; the
    /// 6dd607eb oracle below covers it in bulk).
    #[test]
    fn choice_noise_fixtures() {
        let sampler = CaveBiomeSampler::new(
            800,
            vec![Arc::new(9)],
            vec![Arc::new(0), Arc::new(1), Arc::new(2)],
        );
        assert_eq!(sampler.scale(), 800);
        assert_eq!(sampler.frequency().to_bits(), 983_815_946);
        for (seed, x, z, choice_bits) in [
            (0_i32, 0, 0, 1_056_964_608_u32),
            (0, 1024, -768, 1_060_992_068),
            (123_456_789, -2048, 4096, 1_055_041_553),
            (-987_654_321, 777, -333, 1_047_861_866),
        ] {
            assert_eq!(
                sample(seed.wrapping_add(OFFSET), x, z, sampler.frequency()).to_bits(),
                choice_bits
            );
        }
    }

    /// TerraForged 6dd607eb's choices for the fixtures above: UNIQUE picks by the choice noise, GLOBAL always the one
    /// `terraforged:cave`.
    #[test]
    fn choices_match_terraforged_6dd607eb() {
        let sampler = CaveBiomeSampler::new(
            800,
            vec![Arc::new(9)],
            vec![Arc::new(0), Arc::new(1), Arc::new(2)],
        );
        for (seed, x, z, choice) in [
            (0, 0, 0, 1),
            (0, 1024, -768, 2),
            (123_456_789, -2048, 4096, 1),
            (-987_654_321, 777, -333, 0),
        ] {
            let pick = |cave_type| {
                sampler
                    .get_underground_biome(seed, x, z, cave_type)
                    .map(|value| **value)
            };
            assert_eq!(pick(CaveType::Unique), Some(choice));
            assert_eq!(pick(CaveType::Global), Some(9));
        }
    }

    /// 6dd607eb has no fallback: an empty pool stays empty (Java's null) whatever the other pool holds.
    #[test]
    fn empty_pools_remain_independent() {
        let sampler = CaveBiomeSampler::new(800, vec![Arc::new(9)], Vec::new());
        let pick = |sampler: &CaveBiomeSampler<i32>, cave_type| {
            sampler
                .get_underground_biome(0, 0, 0, cave_type)
                .map(|value| **value)
        };
        assert_eq!(pick(&sampler, CaveType::Unique), None);
        assert_eq!(pick(&sampler, CaveType::Global), Some(9));
        let sampler = CaveBiomeSampler::new(800, Vec::new(), vec![Arc::new(2)]);
        assert_eq!(pick(&sampler, CaveType::Global), None);
        assert_eq!(pick(&sampler, CaveType::Unique), Some(2));
    }

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

    /// `out.txt` of `.local/terraforged/oracles/cave_biome_sampler/6dd607eb/run.sh`: TerraForged 6dd607eb's own
    /// `CaveBiomeSampler` (with its `WeightMap` and `MathUtil`, and Noise2D's `Noise` and `NoiseUtil`) compiled from
    /// source over stub registries, interpreted and with `-Xcomp`. One line per scale and registry: frequency bits,
    /// sample count and an FNV-1a digest of (choice bits, GLOBAL, UNIQUE) per seed and position.
    const TERRAFORGED: &str = "\
S 800 0 983815946 4650 9f2932704b0b4223
S 800 1 983815946 4650 8919c31dddc41cb3
S 800 2 983815946 4650 3a135e419cc868ab
S 800 3 983815946 4650 d3a4788ecba4c4cd
S 800 4 983815946 4650 1b52012d7f7c15a0
S 800 5 983815946 4650 73f8f5aacb79135f
S 1 0 1065353216 4650 1a35e3acda74ce69
S 1 1 1065353216 4650 6b8913c673fec059
S 1 2 1065353216 4650 442aa5e870866079
S 1 3 1065353216 4650 4b559cd1dfcee240
S 1 4 1065353216 4650 c82eb784271370d6
S 1 5 1065353216 4650 d85dc95fb70997e9
S 3 0 1051372203 4650 20cf2a7053734355
S 3 1 1051372203 4650 4b359bf3d9086a25
S 3 2 1051372203 4650 8e4b02eb074affad
S 3 3 1051372203 4650 44c09036838b912f
S 3 4 1051372203 4650 d4ba64b586378a61
S 3 5 1051372203 4650 a14826a5d6508f25
S 1600 0 975427338 4650 ccde786bf8ffc3e8
S 1600 1 975427338 4650 c7210ad9dfef7818
S 1600 2 975427338 4650 d848046cb3b345f0
S 1600 3 975427338 4650 6d644b3e73c4f90e
S 1600 4 975427338 4650 5e262433e8be7387
S 1600 5 975427338 4650 eebf0c20e09d1e22
S 0 0 2139095040 4650 9ef154eec40bc9d5
S 0 1 2139095040 4650 b2a17151fbc8b1cd
S 0 2 2139095040 4650 137464e61c059ff1
S 0 3 2139095040 4650 d2e56e337865a39d
S 0 4 2139095040 4650 58f76e7db7ad4220
S 0 5 2139095040 4650 1a18219bebf8c8a8
S -800 0 -1163667702 4650 411a0a3f2bfbd191
S -800 1 -1163667702 4650 cb38ed62d6e97b91
S -800 2 -1163667702 4650 f631e1e131ed4ef1
S -800 3 -1163667702 4650 8ce91eac28e58315
S -800 4 -1163667702 4650 623740411a1fd786
S -800 5 -1163667702 4650 36d83594808af22a
S -2147483648 0 -1342177280 4650 509fb7594b3d9853
S -2147483648 1 -1342177280 4650 5158b7e7831997d3
S -2147483648 2 -1342177280 4650 86a80388f189a7a3
S -2147483648 3 -1342177280 4650 b319e7889deb20bb
S -2147483648 4 -1342177280 4650 93cb9e70fe83cb4f
S -2147483648 5 -1342177280 4650 ea0ebc5e95cf5416
S 2147483647 0 805306368 4650 9330416e20c07491
S 2147483647 1 805306368 4650 5d263e885f10f731
S 2147483647 2 805306368 4650 40b2663065746611
S 2147483647 3 805306368 4650 fe9b407dd5530b89
S 2147483647 4 805306368 4650 9866995335fe8857
S 2147483647 5 805306368 4650 33190548ec7ea1c5
T 223200";

    #[test]
    fn sampler_matches_terraforged_6dd607eb_java() {
        // The oracle's registries: biome i has id i; {size, has terraforged:cave}. `terraforged:cave` sits at index 1
        // (index 0 in a one-biome registry) and is the GLOBAL pool; UNIQUE is the whole registry, in order.
        let registries = [
            (0, false),
            (1, true),
            (1, false),
            (3, true),
            (4, false),
            (9, true),
        ];
        let scales = [800, 1, 3, 1600, 0, -800, i32::MIN, i32::MAX];
        let mut random = Lcg(162);
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
        let mut lines = TERRAFORGED.lines();
        let mut total = 0;
        for scale in scales {
            for (registry, &(size, has_cave)) in registries.iter().enumerate() {
                let biomes: Vec<_> = (0..size).map(Arc::new).collect();
                let global = if has_cave {
                    vec![Arc::clone(&biomes[1.min(size as usize - 1)])]
                } else {
                    Vec::new()
                };
                let sampler = CaveBiomeSampler::new(scale, global, biomes);
                let (mut hash, mut count) = (0xcbf2_9ce4_8422_2325_u64, 0);
                for &seed in &seeds {
                    for &(x, z) in &positions {
                        let choice = sample(seed.wrapping_add(OFFSET), x, z, sampler.frequency());
                        fold(&mut hash, choice.to_bits() as i32);
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
                    "S {scale} {registry} {} {count} {hash:016x}",
                    sampler.frequency().to_bits() as i32
                );
                assert_eq!(
                    Some(line.as_str()),
                    lines.next(),
                    "scale {scale}, registry {registry}"
                );
            }
        }
        assert_eq!(Some(format!("T {total}").as_str()), lines.next());
        assert_eq!(None, lines.next());
    }
}
