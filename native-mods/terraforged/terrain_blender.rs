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

use super::terraforged::{Domain, Module, Source, SourceConfig, SourceKind, floor, hash_2d};
use super::terraforged_engine::Terrain;
use super::terraforged_terrain_blend_util::{Object2FloatCache, WeightMap, Weighted, random};
use std::sync::Arc;

const REGION_SEED_OFFSET: i32 = 21_491_124;
const WARP_SEED_OFFSET: i32 = 12_678;
const MIN_NOISE: f32 = 5.0 / 255.0;

pub struct TerrainNoise {
    terrain: Terrain,
    weight: f32,
    noise: Module,
}

impl TerrainNoise {
    pub fn new(terrain: Terrain, weight: f32, noise: Module) -> Self {
        let noise = if noise.min_value() < MIN_NOISE {
            noise
                .bias(Module::constant(MIN_NOISE))
                .clamp(Module::constant(0.0), Module::constant(1.0))
        } else {
            noise
        };
        Self {
            terrain,
            weight,
            noise,
        }
    }

    pub fn terrain(&self) -> &Terrain {
        &self.terrain
    }

    pub fn noise(&self) -> &Module {
        &self.noise
    }
}

impl Weighted for TerrainNoise {
    fn weight(&self) -> f32 {
        self.weight
    }
}

pub struct TerrainBlender {
    scale: i32,
    frequency: f32,
    jitter: f32,
    blending: f32,
    warp: Domain,
    terrains: WeightMap<TerrainNoise>,
}

impl TerrainBlender {
    pub fn new(
        seed: i64,
        scale: i32,
        jitter: f32,
        blending: f32,
        terrains: Vec<Arc<TerrainNoise>>,
    ) -> Self {
        let warp_seed = (seed as i32).wrapping_add(WARP_SEED_OFFSET);
        let source = |seed| {
            Module::source(Source::new(
                SourceKind::Simplex,
                SourceConfig {
                    seed,
                    octaves: 3,
                    frequency: 1.0 / scale as f32,
                    ..SourceConfig::default()
                },
            ))
        };
        Self {
            scale,
            frequency: 1.0 / scale as f32,
            jitter,
            blending,
            warp: Domain::domain_warp(
                source(warp_seed),
                source(warp_seed.wrapping_add(1)),
                Module::constant(scale as f32 / 2.5),
            ),
            terrains: WeightMap::of(terrains),
        }
    }

    pub fn scale(&self) -> i32 {
        self.scale
    }

    pub fn value(&self, seed: i32, x: f32, z: f32) -> f32 {
        self.value_with(seed, x, z, &mut Blender::new())
    }

    pub fn value_with(&self, seed: i32, x: f32, z: f32, blender: &mut Blender) -> f32 {
        let region_x = self.warp.x(x, z) * self.frequency;
        let region_z = self.warp.y(x, z) * self.frequency;
        get_cell(
            seed.wrapping_add(REGION_SEED_OFFSET),
            region_x,
            region_z,
            self.jitter,
            blender,
        );
        blender.value(seed, x, z, self.blending, &self.terrains)
    }

    pub fn blender_resource(&self) -> Blender {
        Blender::new()
    }

    pub fn terrain<'a>(&'a self, blender: &Blender) -> &'a Terrain {
        let index = blender.centre_noise_index();
        self.terrains.get_value(index).unwrap().terrain()
    }
}

pub struct Blender {
    closest_index: usize,
    closest_index2: usize,
    hashes: [i32; 9],
    distances: [f32; 9],
    cache: Object2FloatCache<TerrainNoise>,
}

impl Default for Blender {
    fn default() -> Self {
        Self::new()
    }
}

impl Blender {
    pub fn new() -> Self {
        Self {
            closest_index: 0,
            closest_index2: 0,
            hashes: [0; 9],
            distances: [0.0; 9],
            cache: Object2FloatCache::new(9),
        }
    }

    pub fn centre_noise_index(&self) -> f32 {
        self.noise_index(self.closest_index)
    }

    pub fn distance(&self, index: usize) -> f32 {
        self.distances[index].sqrt()
    }

    pub fn centre_value(
        &self,
        _seed: i32,
        x: f32,
        z: f32,
        terrains: &WeightMap<TerrainNoise>,
    ) -> f32 {
        terrains
            .get_value(self.centre_noise_index())
            .unwrap()
            .noise()
            .value(x, z)
    }

    pub fn value(
        &mut self,
        seed: i32,
        x: f32,
        z: f32,
        blending: f32,
        terrains: &WeightMap<TerrainNoise>,
    ) -> f32 {
        let nearest = self.distance(self.closest_index);
        let nearest2 = self.distance(self.closest_index2);
        let border_distance = (nearest + nearest2) * 0.5;
        let blend_radius = border_distance * blending;
        let blend_start = border_distance - blend_radius;
        if nearest <= blend_start {
            self.centre_value(seed, x, z, terrains)
        } else {
            self.blended_value(seed, x, z, nearest, nearest2, blend_radius, terrains)
        }
    }

    pub fn blended_value(
        &mut self,
        _seed: i32,
        x: f32,
        z: f32,
        nearest: f32,
        nearest2: f32,
        blend_range: f32,
        terrains: &WeightMap<TerrainNoise>,
    ) -> f32 {
        self.cache.clear();
        let mut sum_noise = self.cache_value(self.closest_index, x, z, terrains);
        let mut sum_weight = weight(nearest, nearest, blend_range);
        let nearest_weight2 = weight(nearest2, nearest, blend_range);
        if nearest_weight2 > 0.0 {
            sum_noise += self.cache_value(self.closest_index2, x, z, terrains) * nearest_weight2;
            sum_weight += nearest_weight2;
        }
        for index in 0..9 {
            if index == self.closest_index || index == self.closest_index2 {
                continue;
            }
            let sample_weight = weight(self.distance(index), nearest, blend_range);
            if sample_weight > 0.0 {
                sum_noise += self.cache_value(index, x, z, terrains) * sample_weight;
                sum_weight += sample_weight;
            }
        }
        (sum_noise / sum_weight).clamp(0.0, 1.0)
    }

    fn cache_value(
        &mut self,
        index: usize,
        x: f32,
        z: f32,
        terrains: &WeightMap<TerrainNoise>,
    ) -> f32 {
        let terrain = terrains.get_value(self.noise_index(index)).unwrap();
        if let Some(value) = self.cache.get(terrain) {
            return value;
        }
        let value = terrain.noise().value(x, z);
        self.cache.put(terrain, value);
        value
    }

    fn noise_index(&self, index: usize) -> f32 {
        random(self.hashes[index])
    }
}

fn get_cell(seed: i32, x: f32, z: f32, jitter: f32, blender: &mut Blender) {
    let max_x = floor(x).wrapping_add(1);
    let max_z = floor(z).wrapping_add(1);
    let mut nearest_index = 0;
    let mut nearest_index2 = 0;
    let mut nearest_distance = f32::MAX;
    let mut nearest_distance2 = f32::MAX;
    let mut index = 0;
    for cell_z in max_z.wrapping_sub(2)..=max_z {
        for cell_x in max_x.wrapping_sub(2)..=max_x {
            let hash = hash_2d(seed, cell_x, cell_z);
            let delta_x = random(hash_pair(
                hash,
                super::terraforged_terrain_blend_util::X_PRIME,
            ));
            let delta_z = random(hash_pair(
                hash,
                super::terraforged_terrain_blend_util::Y_PRIME,
            ));
            let point_x = cell_x as f32 + delta_x * jitter;
            let point_z = cell_z as f32 + delta_z * jitter;
            let delta_x = x - point_x;
            let delta_z = z - point_z;
            let distance = delta_x * delta_x + delta_z * delta_z;
            blender.hashes[index] = hash;
            blender.distances[index] = distance;
            if distance < nearest_distance {
                nearest_distance2 = nearest_distance;
                nearest_distance = distance;
                nearest_index2 = nearest_index;
                nearest_index = index;
            } else if distance < nearest_distance2 {
                nearest_distance2 = distance;
                nearest_index2 = index;
            }
            index += 1;
        }
    }
    blender.closest_index = nearest_index;
    blender.closest_index2 = nearest_index2;
}

fn hash_pair(first: i32, second: i32) -> i32 {
    let mut hash = first ^ 31_337_i32.wrapping_mul(second);
    hash = hash
        .wrapping_mul(hash)
        .wrapping_mul(hash)
        .wrapping_mul(60_493);
    (hash >> 13) ^ hash
}

fn weight(distance: f32, origin: f32, blend_range: f32) -> f32 {
    let delta = distance - origin;
    if delta <= 0.0 {
        return 1.0;
    }
    if delta >= blend_range {
        return 0.0;
    }
    let weight = 1.0 - delta / blend_range;
    weight * weight
}

#[cfg(test)]
mod tests {
    use super::super::terraforged_engine::TerrainCategory;
    use super::*;

    fn terrain(id: i32, name: &str, weight: f32, value: f32) -> Arc<TerrainNoise> {
        Arc::new(TerrainNoise::new(
            Terrain::new(id, name, TerrainCategory::Flatland),
            weight,
            Module::constant(value),
        ))
    }

    #[test]
    fn terrain_noise_applies_the_source_minimum_bias() {
        let noise = terrain(0, "none", 1.0, 0.0);
        assert_eq!(noise.noise().value(4.0, -7.0).to_bits(), 0x3ca0a0a1);
    }

    #[test]
    fn terrain_blender_matches_terraforged_source() {
        let blender = TerrainBlender::new(
            91,
            64,
            0.8,
            0.65,
            vec![
                terrain(0, "flats", 2.0, 0.2),
                terrain(1, "hills", 3.0, 0.55),
                terrain(2, "mountains", 5.0, 0.9),
            ],
        );
        let points = [(0, 0.0, 0.0), (17, 37.25, -91.5), (-4, -240.0, 118.75)];
        let values: Vec<_> = points
            .map(|(seed, x, z)| blender.value(seed, x, z).to_bits())
            .into_iter()
            .collect();
        assert_eq!(values, [0x3f5cdecb, 0x3f666666, 0x3f666666]);
    }
}
