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

use super::{terraforged_engine::BiomeType, terraforged_terrain_blend_util::WeightMap};
use std::sync::Arc;

pub const TEMPERATURE_BIAS: f32 = 2.0;

pub struct BiomePools<T> {
    pools: [Option<Arc<WeightMap<T>>>; 11],
}

impl<T> Default for BiomePools<T> {
    fn default() -> Self {
        Self {
            pools: std::array::from_fn(|_| None),
        }
    }
}

impl<T> BiomePools<T> {
    pub fn insert(&mut self, climate: BiomeType, pool: Arc<WeightMap<T>>) {
        self.pools[climate as usize] = Some(pool);
    }

    pub fn get(&self, climate: BiomeType) -> Option<&Arc<WeightMap<T>>> {
        self.pools[climate as usize].as_ref()
    }

    pub fn fill_empty_climates(&mut self) -> Vec<BorrowedPool> {
        let donors = self.pools.clone();
        let mut borrowed = Vec::new();
        for climate in BiomeType::VALUES {
            if self.get(climate).is_some_and(|pool| !pool.is_empty()) {
                continue;
            }
            let Some((donor, distance)) = nearest_donor(climate, &donors) else {
                continue;
            };
            self.pools[climate as usize] = donors[donor as usize].clone();
            borrowed.push(BorrowedPool {
                climate,
                donor,
                distance,
            });
        }
        borrowed
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BorrowedPool {
    pub climate: BiomeType,
    pub donor: BiomeType,
    pub distance: f32,
}

fn nearest_donor<T>(
    climate: BiomeType,
    donors: &[Option<Arc<WeightMap<T>>>; 11],
) -> Option<(BiomeType, f32)> {
    let mut nearest = None;
    let mut nearest_distance = f32::MAX;
    for candidate in BiomeType::VALUES {
        let Some(pool) = &donors[candidate as usize] else {
            continue;
        };
        if candidate == climate || pool.is_empty() {
            continue;
        }
        let distance = climate_distance(climate, candidate);
        if distance < nearest_distance {
            nearest_distance = distance;
            nearest = Some(candidate);
        }
    }
    nearest.map(|climate| (climate, nearest_distance))
}

pub fn climate_distance(left: BiomeType, right: BiomeType) -> f32 {
    let mut temperature = midpoint(left.min_temperature(), left.max_temperature())
        - midpoint(right.min_temperature(), right.max_temperature());
    let moisture = midpoint(left.min_moisture(), left.max_moisture())
        - midpoint(right.min_moisture(), right.max_moisture());
    temperature *= TEMPERATURE_BIAS;
    temperature * temperature + moisture * moisture
}

fn midpoint(minimum: f32, maximum: f32) -> f32 {
    (minimum + maximum) * 0.5
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pool(value: &'static str) -> Arc<WeightMap<&'static str>> {
        Arc::new(WeightMap::new(vec![Arc::new(value)], &[1.0]))
    }

    #[test]
    fn empty_climates_borrow_only_from_original_donors() {
        let savanna = pool("savanna");
        let temperate = pool("temperate_forest");
        let tundra = pool("tundra");
        let mut pools = BiomePools::default();
        pools.insert(BiomeType::Savanna, Arc::clone(&savanna));
        pools.insert(BiomeType::TemperateForest, Arc::clone(&temperate));
        pools.insert(BiomeType::Tundra, Arc::clone(&tundra));

        let borrowed = pools.fill_empty_climates();
        let fixtures = [
            (
                BiomeType::TropicalRainforest,
                BiomeType::Savanna,
                1_049_058_483,
            ),
            (BiomeType::Desert, BiomeType::Savanna, 1_033_947_376),
            (
                BiomeType::TemperateRainforest,
                BiomeType::Savanna,
                1_034_858_159,
            ),
            (
                BiomeType::Grassland,
                BiomeType::TemperateForest,
                1_030_287_603,
            ),
            (BiomeType::ColdSteppe, BiomeType::Tundra, 1_049_241_027),
            (BiomeType::Steppe, BiomeType::TemperateForest, 1_039_194_819),
            (BiomeType::Taiga, BiomeType::TemperateForest, 1_045_816_305),
            (BiomeType::Alpine, BiomeType::TemperateForest, 1_033_807_534),
        ];
        assert_eq!(borrowed.len(), fixtures.len());
        for (record, (climate, donor, bits)) in borrowed.iter().zip(fixtures) {
            assert_eq!((record.climate, record.donor), (climate, donor));
            assert_eq!(record.distance.to_bits(), bits);
        }
        assert!(Arc::ptr_eq(
            pools.get(BiomeType::Grassland).unwrap(),
            &temperate
        ));
        assert!(Arc::ptr_eq(
            pools.get(BiomeType::ColdSteppe).unwrap(),
            &tundra
        ));
    }

    /// `BiomeType` ranges after Reimagined's lazy `Lookup` loads (min/max temperature, min/max moisture bits).
    const JAVA_RANGES: [[u32; 4]; 11] = [
        [1_061_866_187, 1_065_353_216, 1_058_905_502, 1_065_353_216],
        [1_055_846_127, 1_065_353_216, 1_042_325_665, 1_059_695_018],
        [1_056_898_815, 1_065_353_216, 0, 1_049_003_655],
        [1_057_787_021, 1_062_392_531, 1_057_787_021, 1_062_392_531],
        [1_054_135_509, 1_062_589_910, 1_043_115_181, 1_058_839_709],
        [1_048_115_449, 1_062_655_703, 1_028_706_513, 1_053_345_993],
        [1_041_272_977, 1_058_444_951, 0, 1_050_056_343],
        [1_053_214_407, 1_061_142_464, 0, 1_040_746_633],
        [1_044_694_213, 1_059_102_881, 1_026_601_137, 1_054_793_439],
        [0, 1_046_010_073, 0, 1_045_746_901],
        [0, 1_065_287_423, 998_277_249, 1_065_353_216],
    ];

    /// Reimagined's `BiomeMapManager.distance(a, b)` bits for every ordered pair, in enum order.
    #[rustfmt::skip]
    const JAVA_DISTANCES: [[u32; 11]; 11] = [
        [0, 1_049_058_483, 1_057_704_780, 1_044_606_490, 1_055_867_416, 1_062_795_352,
         1_070_101_698, 1_064_988_130, 1_067_611_336, 1_077_907_377, 1_060_778_152],
        [1_049_058_483, 0, 1_033_947_376, 1_034_858_159, 1_027_612_549, 1_044_088_400,
         1_058_853_384, 1_046_982_518, 1_055_212_580, 1_070_959_684, 1_047_230_213],
        [1_057_704_780, 1_033_947_376, 0, 1_051_047_753, 1_040_390_577, 1_043_940_817,
         1_058_491_909, 1_040_815_005, 1_056_021_705, 1_070_849_900, 1_053_250_142],
        [1_044_606_490, 1_034_858_159, 1_051_047_753, 0, 1_037_159_626, 1_050_259_269,
         1_060_393_520, 1_054_927_090, 1_057_058_654, 1_071_112_459, 1_043_589_924],
        [1_055_867_416, 1_027_612_549, 1_040_390_577, 1_037_159_626, 0, 1_030_287_603,
         1_051_298_927, 1_039_194_819, 1_045_816_305, 1_066_711_519, 1_033_807_534],
        [1_062_795_352, 1_044_088_400, 1_043_940_817, 1_050_259_269, 1_030_287_603, 0,
         1_040_530_936, 1_021_133_221, 1_032_717_696, 1_061_524_066, 1_034_961_366],
        [1_070_101_698, 1_058_853_384, 1_058_491_909, 1_060_393_520, 1_051_298_927, 1_040_530_936,
         0, 1_043_395_126, 1_015_560_166, 1_049_241_027, 1_045_023_694],
        [1_064_988_130, 1_046_982_518, 1_040_815_005, 1_054_927_090, 1_039_194_819, 1_021_133_221,
         1_043_395_126, 0, 1_040_568_090, 1_062_969_512, 1_045_852_173],
        [1_067_611_336, 1_055_212_580, 1_056_021_705, 1_057_058_654, 1_045_816_305, 1_032_717_696,
         1_015_560_166, 1_040_568_090, 0, 1_053_199_056, 1_036_855_688],
        [1_077_907_377, 1_070_959_684, 1_070_849_900, 1_071_112_459, 1_066_711_519, 1_061_524_066,
         1_049_241_027, 1_062_969_512, 1_053_199_056, 0, 1_061_554_574],
        [1_060_778_152, 1_047_230_213, 1_053_250_142, 1_043_589_924, 1_033_807_534, 1_034_961_366,
         1_045_023_694, 1_045_852_173, 1_036_855_688, 1_061_554_574, 0],
    ];

    /// `BiomePoolOracle` (`.local/terraforged/oracles/biome_pool/`, `run.sh`): Reimagined's verbatim
    /// `fillEmptyClimates` over all 3^11 absent/empty/non-empty combinations.
    const JAVA_CASES: u32 = 177_147;
    const JAVA_HASH: u64 = 16_007_713_335_377_952_318;
    const JAVA_SAMPLES: [(u32, [i32; 11]); 7] = [
        (0, [-1; 11]),
        (1, [-2, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1]),
        (2, [0; 11]),
        (12_345, [1, 1, 1, 1, 4, 5, 5, 7, 5, 5, 4]),
        (88_573, [-2; 11]),
        (177_143, [0, 4, 2, 3, 4, 5, 6, 7, 8, 9, 10]),
        (177_146, [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10]),
    ];

    /// The oracle's pool for a type: state 1 = no values, state 2 = `1 + ordinal % 3` biomes with weights that vary
    /// by `ordinal % 4` (weights never decide emptiness).
    fn oracle_pool(ordinal: usize, state: u32) -> Arc<WeightMap<usize>> {
        if state == 1 {
            return Arc::new(WeightMap::new(Vec::new(), &[]));
        }
        let values = (0..1 + ordinal % 3).map(Arc::new).collect();
        let weights: &[f32] = match ordinal % 4 {
            0 => &[1.0, 1.0, 1.0],
            1 => &[f32::NAN],
            2 => &[0.0, 0.0],
            _ => &[],
        };
        Arc::new(WeightMap::new(values, weights))
    }

    #[test]
    fn ranges_and_distances_match_reimagined_java() {
        for climate in BiomeType::VALUES {
            let range = [
                climate.min_temperature(),
                climate.max_temperature(),
                climate.min_moisture(),
                climate.max_moisture(),
            ]
            .map(f32::to_bits);
            assert_eq!(range, JAVA_RANGES[climate as usize], "{climate:?}");
            for other in BiomeType::VALUES {
                assert_eq!(
                    climate_distance(climate, other).to_bits(),
                    JAVA_DISTANCES[climate as usize][other as usize],
                    "{climate:?} {other:?}"
                );
            }
        }
    }

    #[test]
    fn fill_matches_reimagined_java() {
        let mut hash = 0xcbf2_9ce4_8422_2325_u64;
        let mut samples = Vec::new();
        for case in 0..JAVA_CASES {
            let mut pools = BiomePools::default();
            let mut originals = Vec::new();
            let mut rest = case;
            for climate in BiomeType::VALUES {
                let state = rest % 3;
                rest /= 3;
                let pool = (state != 0).then(|| oracle_pool(climate as usize, state));
                if let Some(pool) = &pool {
                    pools.insert(climate, Arc::clone(pool));
                }
                originals.push(pool);
            }
            pools.fill_empty_climates();
            let codes = BiomeType::VALUES.map(|climate| match pools.get(climate) {
                None => -1,
                Some(pool) => {
                    let donor = originals
                        .iter()
                        .position(|original| {
                            original.as_ref().is_some_and(|o| Arc::ptr_eq(o, pool))
                        })
                        .map_or(-3, |index| index as i32);
                    if donor == climate as i32 && pool.is_empty() {
                        -2
                    } else {
                        donor
                    }
                }
            });
            for code in codes {
                for byte in code.to_le_bytes() {
                    hash ^= u64::from(byte);
                    hash = hash.wrapping_mul(0x0100_0000_01b3);
                }
            }
            if JAVA_SAMPLES.iter().any(|(sample, _)| *sample == case) {
                samples.push((case, codes));
            }
        }
        assert_eq!(samples, JAVA_SAMPLES);
        assert_eq!(hash, JAVA_HASH);
    }

    #[test]
    fn all_empty_climates_remain_empty() {
        let mut pools = BiomePools::<()>::default();
        assert!(pools.fill_empty_climates().is_empty());
        assert!(
            BiomeType::VALUES
                .into_iter()
                .all(|climate| pools.get(climate).is_none())
        );
    }
}
