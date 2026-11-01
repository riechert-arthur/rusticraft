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
    terraforged_engine::{Terrain, TerrainRegistry},
    terraforged_terrain_levels::TerrainLevels,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Index {
    offset: i32,
    size: i32,
    chunk: bool,
}

impl Index {
    pub const CHUNK: Self = Self {
        offset: 0,
        size: 16,
        chunk: true,
    };

    pub const fn bordered_chunk(border: i32) -> Self {
        Self {
            offset: border,
            size: 16 + border * 2,
            chunk: false,
        }
    }

    pub fn of(self, x: i32, z: i32) -> usize {
        if self.chunk {
            (((z & 15) << 4) | (x & 15)) as usize
        } else {
            let x = x.wrapping_add(self.offset);
            let z = z.wrapping_add(self.offset);
            z.wrapping_mul(self.size).wrapping_add(x) as usize
        }
    }

    pub const fn length(self) -> usize {
        (self.size * self.size) as usize
    }
}

pub struct FloatMap {
    index: Index,
    data: Vec<f32>,
}

impl FloatMap {
    pub fn new() -> Self {
        Self::with_index(Index::CHUNK)
    }

    pub fn bordered(border: i32) -> Self {
        Self::with_index(Index::bordered_chunk(border))
    }

    fn with_index(index: Index) -> Self {
        Self {
            index,
            data: vec![0.0; index.length()],
        }
    }

    pub const fn index(&self) -> Index {
        self.index
    }

    pub fn get(&self, x: i32, z: i32) -> f32 {
        self.get_index(self.index.of(x, z))
    }

    pub fn set(&mut self, x: i32, z: i32, value: f32) {
        self.set_index(self.index.of(x, z), value);
    }

    pub fn get_index(&self, index: usize) -> f32 {
        self.data[index]
    }

    pub fn set_index(&mut self, index: usize, value: f32) {
        self.data[index] = value;
    }
}

impl Default for FloatMap {
    fn default() -> Self {
        Self::new()
    }
}

pub struct ObjectMap<T> {
    index: Index,
    data: Vec<Option<T>>,
}

impl<T> ObjectMap<T> {
    pub fn new() -> Self {
        Self::with_index(Index::CHUNK)
    }

    pub fn bordered(border: i32) -> Self {
        Self::with_index(Index::bordered_chunk(border))
    }

    fn with_index(index: Index) -> Self {
        Self {
            index,
            data: std::iter::repeat_with(|| None)
                .take(index.length())
                .collect(),
        }
    }

    pub const fn index(&self) -> Index {
        self.index
    }

    pub fn get(&self, x: i32, z: i32) -> Option<&T> {
        self.get_index(self.index.of(x, z))
    }

    pub fn set(&mut self, x: i32, z: i32, value: T) {
        self.set_index(self.index.of(x, z), value);
    }

    pub fn get_index(&self, index: usize) -> Option<&T> {
        self.data[index].as_ref()
    }

    pub fn set_index(&mut self, index: usize, value: T) {
        self.data[index] = Some(value);
    }

    pub fn fill(&mut self, mut supplier: impl FnMut() -> T) {
        for value in &mut self.data {
            *value = Some(supplier());
        }
    }
}

impl<T> Default for ObjectMap<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone)]
pub struct NoiseSample {
    pub continent_center: i64,
    pub continent_noise: f32,
    pub base_noise: f32,
    pub height_noise: f32,
    pub river_noise: f32,
    pub terrain_type: Terrain,
}

impl NoiseSample {
    pub fn reset(&mut self) -> &mut Self {
        *self = Self::default();
        self
    }

    pub fn copy(&mut self, other: &Self) -> &mut Self {
        self.clone_from(other);
        self
    }
}

impl Default for NoiseSample {
    fn default() -> Self {
        Self {
            continent_center: 0,
            continent_noise: 1.0,
            base_noise: 0.0,
            height_noise: 0.0,
            river_noise: 1.0,
            terrain_type: TerrainRegistry::new().get_by_id(0),
        }
    }
}

pub struct NoiseData {
    sample: NoiseSample,
    height: FloatMap,
    river: FloatMap,
    base: FloatMap,
    terrain: ObjectMap<Terrain>,
}

impl NoiseData {
    pub const BORDER: i32 = 1;
    pub const MIN: i32 = -Self::BORDER;
    pub const MAX: i32 = 16 + Self::BORDER;

    pub fn new() -> Self {
        Self {
            sample: NoiseSample::default(),
            height: FloatMap::bordered(Self::BORDER),
            river: FloatMap::bordered(Self::BORDER),
            base: FloatMap::bordered(Self::BORDER),
            terrain: ObjectMap::bordered(Self::BORDER),
        }
    }

    pub fn sample_mut(&mut self) -> &mut NoiseSample {
        &mut self.sample
    }

    pub const fn height(&self) -> &FloatMap {
        &self.height
    }

    pub const fn base(&self) -> &FloatMap {
        &self.base
    }

    pub const fn river(&self) -> &FloatMap {
        &self.river
    }

    pub const fn terrain(&self) -> &ObjectMap<Terrain> {
        &self.terrain
    }

    pub fn set_noise(&mut self, x: i32, z: i32, sample: &NoiseSample) {
        self.set_noise_index(self.height.index().of(x, z), sample);
    }

    pub fn set_noise_index(&mut self, index: usize, sample: &NoiseSample) {
        self.terrain.set_index(index, sample.terrain_type.clone());
        self.height.set_index(index, sample.height_noise);
        self.base.set_index(index, sample.base_noise);
        self.river.set_index(index, sample.river_noise);
    }

    pub fn is_inside_chunk(x: i32, z: i32) -> bool {
        (-1..=16).contains(&x) && (-1..=16).contains(&z)
    }
}

impl Default for NoiseData {
    fn default() -> Self {
        Self::new()
    }
}

pub struct TerrainData {
    levels: TerrainLevels,
    height: FloatMap,
    gradient: FloatMap,
    river: FloatMap,
    base_height: FloatMap,
    terrain: ObjectMap<Terrain>,
    min: f32,
    max: f32,
    max_base: f32,
}

impl TerrainData {
    pub fn new(levels: TerrainLevels) -> Self {
        Self {
            levels,
            height: FloatMap::new(),
            gradient: FloatMap::new(),
            river: FloatMap::new(),
            base_height: FloatMap::new(),
            terrain: ObjectMap::new(),
            min: f32::MAX,
            max: f32::from_bits(1),
            max_base: f32::from_bits(1),
        }
    }

    pub fn min(&self) -> i32 {
        self.levels.height(self.min)
    }

    pub fn max(&self) -> i32 {
        self.levels.height(self.max)
    }

    pub fn max_base(&self) -> i32 {
        self.levels.height(self.max_base)
    }

    pub fn height_at(&self, x: i32, z: i32) -> i32 {
        self.levels.height(self.height.get(x, z))
    }

    pub fn base_height_at(&self, x: i32, z: i32) -> i32 {
        self.levels.height(self.base_height.get(x, z))
    }

    pub const fn levels(&self) -> TerrainLevels {
        self.levels
    }

    pub const fn height(&self) -> &FloatMap {
        &self.height
    }

    pub const fn base_height(&self) -> &FloatMap {
        &self.base_height
    }

    pub const fn gradient(&self) -> &FloatMap {
        &self.gradient
    }

    pub const fn river(&self) -> &FloatMap {
        &self.river
    }

    pub const fn terrain(&self) -> &ObjectMap<Terrain> {
        &self.terrain
    }

    pub fn normalized_gradient(&self, x: i32, z: i32, norm: f32) -> f32 {
        (self.gradient.get(x, z) * norm).clamp(0.0, 1.0)
    }

    pub fn accept(&mut self, noise_data: &NoiseData) {
        for z in 0..16 {
            for x in 0..16 {
                let height_noise = noise_data.height.get(x, z);
                let base_level_noise = noise_data.base.get(x, z);
                let scaled_height = self.levels.scaled_height(height_noise);
                let scaled_base_level = self.levels.scaled_base_level(base_level_noise);
                let terrain_type = noise_data
                    .terrain
                    .get(x, z)
                    .expect("generated noise has terrain");
                let north = noise_data.height.get(x, z - 1);
                let south = noise_data.height.get(x, z + 1);
                let east = noise_data.height.get(x + 1, z);
                let west = noise_data.height.get(x - 1, z);
                let dx = east - west;
                let dz = south - north;
                let noise_gradient = (dx * dx + dz * dz).sqrt().clamp(0.0, 1.0);

                self.gradient.set(x, z, noise_gradient);
                self.terrain.set(x, z, terrain_type.clone());
                self.height.set(x, z, scaled_height);
                self.base_height.set(x, z, scaled_base_level);
                self.river.set(x, z, noise_data.river.get(x, z));
                self.max = self.max.max(scaled_height);
                self.min = self.min.min(scaled_height);
                self.max_base = self.max_base.max(scaled_base_level);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storage_maps_match_terraforged_source() {
        let mut chunk = FloatMap::new();
        chunk.set(-1, -1, 0.25);
        assert_eq!(chunk.get(15, 15), 0.25);
        let mut bordered = FloatMap::bordered(1);
        bordered.set(-1, -1, 0.5);
        bordered.set(16, 16, 0.75);
        assert_eq!(bordered.get_index(0), 0.5);
        assert_eq!(bordered.get_index(323), 0.75);
        assert!(NoiseData::is_inside_chunk(-1, 16));
        assert!(!NoiseData::is_inside_chunk(-2, 16));
    }

    #[test]
    fn noise_and_terrain_data_match_terraforged_source() {
        let terrains = TerrainRegistry::new();
        let mut noise = NoiseData::new();
        for z in NoiseData::MIN..NoiseData::MAX {
            for x in NoiseData::MIN..NoiseData::MAX {
                let sample = NoiseSample {
                    height_noise: (x * 3 + z * 5 + 100) as f32 / 512.0,
                    base_noise: (x - z + 20) as f32 / 64.0,
                    river_noise: if (x + z) & 1 == 0 { 0.0 } else { 1.0 },
                    terrain_type: terrains.get_by_id(8),
                    ..NoiseSample::default()
                };
                noise.set_noise(x, z, &sample);
            }
        }
        let mut terrain = TerrainData::new(TerrainLevels::default());
        terrain.accept(&noise);

        assert_eq!(terrain.min(), 93);
        assert_eq!(terrain.max(), 206);
        assert_eq!(terrain.max_base(), 132);
        assert_eq!(terrain.height.get(0, 0).to_bits(), 0x42bb8000);
        assert_eq!(terrain.base_height.get(7, 9).to_bits(), 0x42c40000);
        assert_eq!(terrain.gradient.get(15, 15).to_bits(), 0x3cba9728);
        assert_eq!(terrain.normalized_gradient(0, 0, 2.0).to_bits(), 0x3d3a9728);
        assert_eq!(terrain.river.get(7, 9), 0.0);
        assert_eq!(terrain.terrain.get(0, 0).unwrap().name(), "flats");
    }
}
