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
