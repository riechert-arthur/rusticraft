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

// `BiomeMapManager.getBiomeMap()`: one weighted pool per climate. TerraForged never
// borrows another climate's biomes; an empty pool makes the sampler use plains.
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
}
