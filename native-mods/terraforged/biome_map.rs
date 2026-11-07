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

// TerraForged 6dd607eb's `BiomeMapManager.getBiomeMap()` map
// (`src/main/java/com/terraforged/mod/worldgen/biome/util/BiomeMapManager.java`): one weighted pool per climate. It
// never borrows another climate's pool; an empty pool has no biome to give (`getInitialBiome` then returns plains).

use super::{terraforged_engine::BiomeType, terraforged_terrain_blend_util::WeightMap};
use std::sync::Arc;

/// One biome pool per climate type (`Map<BiomeType, WeightMap<Holder<Biome>>>`, an `EnumMap`): `None` is a type
/// with no entry.
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
    /// `map.put(climate, pool)`.
    pub fn insert(&mut self, climate: BiomeType, pool: Arc<WeightMap<T>>) {
        self.pools[climate as usize] = Some(pool);
    }

    /// `map.get(climate)`.
    pub fn get(&self, climate: BiomeType) -> Option<&Arc<WeightMap<T>>> {
        self.pools[climate as usize].as_ref()
    }

    /// Whether any pool holds `biome` (no Java counterpart: `/locate biome` asks it).
    pub fn holds(&self, biome: &T) -> bool
    where
        T: PartialEq,
    {
        self.pools
            .iter()
            .flatten()
            .any(|pool| pool.values().iter().any(|value| **value == *biome))
    }
}
