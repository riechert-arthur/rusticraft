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

use super::terraforged::{Domain, Module, Source, SourceConfig, SourceKind, clamp, floor, hash_2d};
use super::terraforged_engine::Terrain;
use super::terraforged_terrain_blend_util::{
    Object2FloatCache, PositionFinder, SpiralIterator, WeightMap, Weighted, X_PRIME, Y_PRIME,
    random,
};
use super::terraforged_util::{
    pack_floats, unpack_left, unpack_left_float, unpack_right, unpack_right_float,
};
use std::cell::RefCell;
use std::sync::Arc;

/// `TerrainBlender.REGION_SEED_OFFSET`, added to the world seed's low 32 bits for the cell hashes.
const REGION_SEED_OFFSET: i32 = 21_491_124;
/// `TerrainBlender.WARP_SEED_OFFSET`, added to the world seed's low 32 bits for the warp noises.
const WARP_SEED_OFFSET: i32 = 12_678;
/// `TerrainNoise.MIN_NOISE`, a `double` (5 / 255): noises reporting a lower minimum are raised by it.
const MIN_NOISE: f64 = 5.0 / 255.0;

/// TerraForged's `worldgen.asset.TerrainNoise` (1.18.2-0.3.1): a terrain, its selection weight and its height
/// noise. The jar holds the terrain through a registry `Holder<TerrainType>`; here it is the terrain itself.
pub struct TerrainNoise {
    terrain: Terrain,
    weight: f32,
    noise: Module,
}

impl TerrainNoise {
    /// `new TerrainNoise(type, weight, noise)`: a noise whose `minValue` (widened to `double`) is below
    /// `MIN_NOISE` becomes `noise.bias(MIN_NOISE).clamp(0, 1)` (`Module.bias(double)`/`clamp(double, double)`, so the
    /// bias is `(float) MIN_NOISE`); a NaN minimum keeps the noise unchanged.
    pub fn new(terrain: Terrain, weight: f32, noise: Module) -> Self {
        let noise = if f64::from(noise.min_value()) < MIN_NOISE {
            noise
                .bias(Module::constant(MIN_NOISE as f32))
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

    /// `terrain()`.
    pub fn terrain(&self) -> &Terrain {
        &self.terrain
    }

    /// `noise()`.
    pub fn noise(&self) -> &Module {
        &self.noise
    }

    /// `withSeed(seed)`: a new terrain noise with the same terrain and weight and the noise reseeded
    /// ([`Module::with_seed`]), through the constructor again. Reseeding reads a raised noise's bias back as 0, so
    /// the clamped result reports a minimum of 0 and is raised once more.
    pub fn with_seed(&self, seed: i64) -> Self {
        Self::new(
            self.terrain.clone(),
            self.weight,
            self.noise.with_seed(seed),
        )
    }
}

impl Weighted for TerrainNoise {
    /// `weight()`.
    fn weight(&self) -> f32 {
        self.weight
    }
}

/// TerraForged's `worldgen.terrain.TerrainBlender` (1.18.2-0.3.1), the terrain module of the mod's noise
/// generator: the point is warped and scaled into a grid of jittered cells; each cell's hash picks a terrain from the
/// weight map, and points near the border of their nearest cell blend in the neighbouring cells' terrain noises.
/// As in the jar, the seed is bound when the blender is built; [`TerrainBlender::with_seed`] builds a new one.
pub struct TerrainBlender {
    region_seed: i32,
    scale: i32,
    frequency: f32,
    jitter: f32,
    blending: f32,
    warp: Domain,
    terrains: WeightMap<TerrainNoise>,
}

impl TerrainBlender {
    /// `new TerrainBlender(seed, scale, jitter, blending, terrains)`: the cell seed is `(int) seed +
    /// REGION_SEED_OFFSET`, the frequency `1f / scale`, and the warp `Domain.warp(Source.SIMPLEX, (int) seed +
    /// WARP_SEED_OFFSET, scale, 3, scale / 2.5f)` (two 3-octave simplex noises, the second seeded one higher), all in
    /// wrapping Java `int` math.
    pub fn new(
        seed: i64,
        scale: i32,
        jitter: f32,
        blending: f32,
        terrains: Vec<Arc<TerrainNoise>>,
    ) -> Self {
        let warp_seed = (seed as i32).wrapping_add(WARP_SEED_OFFSET);
        let frequency = 1.0 / scale as f32;
        let source = |seed| {
            Module::source(Source::new(
                SourceKind::Simplex,
                SourceConfig {
                    seed,
                    octaves: 3,
                    frequency,
                    ..SourceConfig::default()
                },
            ))
        };
        Self {
            region_seed: (seed as i32).wrapping_add(REGION_SEED_OFFSET),
            scale,
            frequency,
            jitter,
            blending,
            terrains: WeightMap::of(terrains),
            warp: Domain::domain_warp(
                source(warp_seed),
                source(warp_seed.wrapping_add(1)),
                Module::constant(scale as f32 / 2.5),
            ),
        }
    }

    /// `withSeed(seed)`: `new TerrainBlender(seed, scale, jitter, blending, reseeded)`, each terrain noise reseeded in
    /// the weight map's order ([`TerrainNoise::with_seed`]). This blender is left as it is.
    pub fn with_seed(&self, seed: i64) -> Self {
        let terrains = self
            .terrains
            .values()
            .iter()
            .map(|terrain| Arc::new(terrain.with_seed(seed)))
            .collect();
        Self::new(seed, self.scale, self.jitter, self.blending, terrains)
    }

    /// `getValue(x, z)`, with this thread's [`Blender`] (`localBlender`; one per thread is shared by all
    /// blenders, which is the same since `getValue` overwrites all its state). Its cache is cleared afterwards, so
    /// it holds no terrain past the call; a nested call on the same thread uses a fresh blender.
    pub fn value(&self, x: f32, z: f32) -> f32 {
        thread_local! {
            static BLENDER: RefCell<Blender> = RefCell::new(Blender::new());
        }
        BLENDER.with(|blender| match blender.try_borrow_mut() {
            Ok(mut blender) => {
                let value = self.value_with(x, z, &mut blender);
                blender.cache.clear();
                value
            }
            Err(_) => self.value_with(x, z, &mut Blender::new()),
        })
    }

    /// `getValue(x, z, blender)`: finds the point's cells, then blends. Panics where Java throws: when no cell's
    /// squared distance is below `Float.MAX_VALUE` (NaN, infinite or huge coordinates), or when the weight map has no
    /// terrain for a cell (an empty map, NaN or negative weights).
    pub fn value_with(&self, x: f32, z: f32, blender: &mut Blender) -> f32 {
        let region_x = self.warp.x(x, z) * self.frequency;
        let region_z = self.warp.y(x, z) * self.frequency;
        get_cell(self.region_seed, region_x, region_z, self.jitter, blender);
        blender.value(x, z, self.blending, &self.terrains)
    }

    /// `getBlenderResource()`: a fresh [`Blender`].
    pub fn blender_resource(&self) -> Blender {
        Blender::new()
    }

    /// `getTerrain(blender)`: the terrain of the blender's nearest cell (from the last `value_with`).
    pub fn terrain<'a>(&'a self, blender: &Blender) -> &'a Terrain {
        terrain_at(&self.terrains, blender.centre_noise_index()).terrain()
    }

    /// `findNearest(x, z, minRadius, maxRadius, terrain)`: `None` (Java's null) when no terrain noise has the
    /// terrain's name (`String.equals`, the first match); otherwise a finder over [`TerrainBlender::iterator`] whose
    /// `next` walks on to the next cell in that noise's weight band (`WeightMap.getBand`, by identity) and returns its
    /// jittered point divided by the frequency (back to blocks), `PosUtil.packf`ed. When the spiral runs out during
    /// a search, the point is Java's `0L` divided the same way (a zero, negative for a negative scale); once it is
    /// out, `next` is `None` (Java's `0L`). Java's spiral quirks are kept: a `min_radius` above `max_radius` revisits
    /// one cell forever, so a search there that misses never returns, as in Java, and a negative `min_radius` panics
    /// when the walk steps into ring 0, where Java divides by zero. The finder holds no borrow of the blender (it copies the seed,
    /// jitter, frequency and band), so it can outlive it or move to another thread.
    pub fn find_nearest(
        &self,
        x: f32,
        z: f32,
        min_radius: i32,
        max_radius: i32,
        terrain: &Terrain,
    ) -> Option<PositionFinder<impl FnMut(&mut SpiralIterator) -> i64 + use<>>> {
        let noise = self
            .terrains
            .find(|candidate| candidate.terrain().name() == terrain.name())?;
        let band = self.terrains.band(noise);
        let lower = unpack_left_float(band);
        let upper = unpack_right_float(band);
        let (seed, jitter, frequency) = (self.region_seed, self.jitter, self.frequency);
        Some(
            self.iterator(x, z, min_radius, max_radius)
                .finder(move |spiral| {
                    let position = find(seed, jitter, lower, upper, spiral);
                    pack_floats(
                        unpack_left_float(position) / frequency,
                        unpack_right_float(position) / frequency,
                    )
                }),
        )
    }

    /// `iterator(x, z, minRadius, maxRadius)`: the [`SpiralIterator`] of cells around the point's warped,
    /// scaled cell (`NoiseUtil.floor`, as `getValue` locates it).
    pub fn iterator(&self, x: f32, z: f32, min_radius: i32, max_radius: i32) -> SpiralIterator {
        let region_x = self.warp.x(x, z) * self.frequency;
        let region_z = self.warp.y(x, z) * self.frequency;
        SpiralIterator::new(floor(region_x), floor(region_z), min_radius, max_radius)
    }
}

/// `TerrainBlender.Blender`: the nine cells around the last point (hashes and squared distances), its nearest two
/// and the terrain-sample cache of one blend. A closest index is -1 (as in Java) when no cell was near enough.
pub struct Blender {
    closest_index: i32,
    closest_index2: i32,
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
    /// `new Blender()`.
    pub fn new() -> Self {
        Self {
            closest_index: 0,
            closest_index2: 0,
            hashes: [0; 9],
            distances: [0.0; 9],
            cache: Object2FloatCache::new(9),
        }
    }

    /// `closestIndex`: the nearest cell (0..9, row by row from the lower corner), or -1.
    pub fn closest_index(&self) -> i32 {
        self.closest_index
    }

    /// `closestIndex2`: the second nearest cell, or -1.
    pub fn closest_index2(&self) -> i32 {
        self.closest_index2
    }

    /// `getCentreNoiseIndex()`: the nearest cell's 0..1 terrain selector, `MathUtil.rand(hash)`.
    pub fn centre_noise_index(&self) -> f32 {
        self.noise_index(self.closest_index)
    }

    /// `getDistance(index)`: `NoiseUtil.sqrt` of the cell's squared distance. Panics for an index outside 0..9 (-1
    /// included) where Java throws `ArrayIndexOutOfBoundsException`.
    pub fn distance(&self, index: i32) -> f32 {
        self.distances[slot(index)].sqrt()
    }

    /// `getCentreValue(x, z, terrains)`: the nearest cell's terrain noise, uncached.
    pub fn centre_value(&self, x: f32, z: f32, terrains: &WeightMap<TerrainNoise>) -> f32 {
        terrain_at(terrains, self.centre_noise_index())
            .noise()
            .value(x, z)
    }

    /// `getValue(x, z, blending, terrains)`: the nearest cell's noise when the point is closer to its cell than
    /// `(1 - blending)` of the way to the border with the second nearest (`nearest <= blendStart`, so NaN blends),
    /// otherwise [`Blender::blended_value`] over a blend range of `blending` times that border distance.
    pub fn value(
        &mut self,
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
            self.centre_value(x, z, terrains)
        } else {
            self.blended_value(x, z, nearest, nearest2, blend_radius, terrains)
        }
    }

    /// `getBlendedValue(x, z, nearest, nearest2, blendRange, terrains)`: clears the cache, then sums the nearest
    /// cell's noise (weight `getWeight(nearest, nearest, range)`), the second nearest's and every other cell's (in
    /// index order) whose weight is above 0, each sample through the cache, and returns `NoiseUtil.clamp(sum /
    /// weights, 0, 1)` (NaN passes through).
    pub fn blended_value(
        &mut self,
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
        clamp(sum_noise / sum_weight, 0.0, 1.0)
    }

    /// `getCacheValue(index, x, z, terrains)`: the cell's terrain sample from the cache; a NaN there (missing, or
    /// a NaN sample) is sampled again and put back, as Java's `Float.isNaN` test does.
    fn cache_value(
        &mut self,
        index: i32,
        x: f32,
        z: f32,
        terrains: &WeightMap<TerrainNoise>,
    ) -> f32 {
        let terrain = terrain_at(terrains, self.noise_index(index));
        let mut value = self.cache.get(terrain);
        if value.is_nan() {
            value = terrain.noise().value(x, z);
            self.cache.put(terrain, value);
        }
        value
    }

    /// `getNoiseIndex(index)`: `MathUtil.rand(hashes[index])`.
    fn noise_index(&self, index: i32) -> f32 {
        random(self.hashes[slot(index)])
    }
}

/// A Java array index: panics for one outside the array (-1 included) where Java throws.
fn slot(index: i32) -> usize {
    usize::try_from(index).expect("cell index -1: Java throws ArrayIndexOutOfBoundsException")
}

/// `(TerrainNoise) terrains.getValue(noise)`, dereferenced: panics where Java's null throws.
fn terrain_at(terrains: &WeightMap<TerrainNoise>, noise: f32) -> &Arc<TerrainNoise> {
    terrains
        .get_value(noise)
        .expect("no terrain for the cell: Java throws NullPointerException")
}

/// `TerrainBlender.getCell(seed, x, z, jitter, blender)`: the 3x3 cells from `floor - 1` to `floor + 1` (Java
/// `int` math, `NoiseUtil.floor`), row by row; each cell's point is `cell + MathUtil.rand(hash, prime) * jitter`
/// and its squared distance `NoiseUtil.dist2`. Strictly closer cells win (ties keep the earlier one); with no
/// distance below `Float.MAX_VALUE` both closest indices stay -1.
fn get_cell(seed: i32, x: f32, z: f32, jitter: f32, blender: &mut Blender) {
    let max_x = floor(x).wrapping_add(1);
    let max_z = floor(z).wrapping_add(1);
    let mut nearest_index = -1;
    let mut nearest_index2 = -1;
    let mut nearest_distance = f32::MAX;
    let mut nearest_distance2 = f32::MAX;
    let mut index = 0;
    // Java's `for (cz = maxZ - 2; cz <= maxZ; cz++)`: three rows, or none when `maxZ - 2` wraps past `maxZ`.
    let mut cell_z = max_z.wrapping_sub(2);
    while cell_z <= max_z {
        let mut cell_x = max_x.wrapping_sub(2);
        while cell_x <= max_x {
            let hash = hash_2d(seed, cell_x, cell_z);
            let point_x = cell_x as f32 + random2(hash, X_PRIME) * jitter;
            let point_z = cell_z as f32 + random2(hash, Y_PRIME) * jitter;
            let delta_x = x - point_x;
            let delta_z = z - point_z;
            let distance = delta_x * delta_x + delta_z * delta_z;
            blender.hashes[slot(index)] = hash;
            blender.distances[slot(index)] = distance;
            if distance < nearest_distance {
                nearest_distance2 = nearest_distance;
                nearest_distance = distance;
                nearest_index2 = nearest_index;
                nearest_index = index;
            } else if distance < nearest_distance2 {
                nearest_distance2 = distance;
                nearest_index2 = index;
            }
            cell_x = cell_x.wrapping_add(1);
            index += 1;
        }
        cell_z = cell_z.wrapping_add(1);
    }
    blender.closest_index = nearest_index;
    blender.closest_index2 = nearest_index2;
}

/// `MathUtil.rand(seed, offset)`: `MathUtil.rand(int)` of `NoiseUtil.hash(seed, offset)`.
fn random2(seed: i32, offset: i32) -> f32 {
    let mut hash = seed ^ 31_337_i32.wrapping_mul(offset);
    hash = hash
        .wrapping_mul(hash)
        .wrapping_mul(hash)
        .wrapping_mul(60_493);
    random((hash >> 13) ^ hash)
}

/// `TerrainBlender.find(seed, jitter, lower, upper, spiral)`: walks the spiral (while `hasNext`) to the first
/// cell whose `MathUtil.rand(hash)` is in the band, strictly above `lower` and at most `upper` (so NaN never matches),
/// and returns its jittered point `PosUtil.packf(cell + MathUtil.rand(hash, prime) * jitter)`; `0L` once the spiral
/// runs out.
fn find(seed: i32, jitter: f32, lower: f32, upper: f32, spiral: &mut SpiralIterator) -> i64 {
    for position in spiral {
        let cell_x = unpack_left(position);
        let cell_z = unpack_right(position);
        let hash = hash_2d(seed, cell_x, cell_z);
        let noise = random(hash);
        if noise > lower && noise <= upper {
            let point_x = cell_x as f32 + random2(hash, X_PRIME) * jitter;
            let point_z = cell_z as f32 + random2(hash, Y_PRIME) * jitter;
            return pack_floats(point_x, point_z);
        }
    }
    0
}

/// `TerrainBlender.Blender.getWeight(distance, origin, blendRange)`: 1 up to the origin, 0 from `blendRange` past
/// it, `(1 - delta / blendRange)^2` between (NaN comparisons fall through to the formula, as in Java).
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
    use super::super::terraforged_engine::{TerrainCategory, TerrainRegistry};
    use super::*;
    use std::fmt::Write;
    use std::panic::{AssertUnwindSafe, catch_unwind};

    /// `Float.floatToIntBits`: every NaN prints as 7fc00000.
    fn h(v: f32) -> String {
        format!("{:08x}", if v.is_nan() { 0x7fc0_0000 } else { v.to_bits() })
    }

    fn fnv(text: &str) -> u64 {
        text.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
        })
    }

    /// `(float) (5.0 / 255.0)`.
    const BOUND: f32 = MIN_NOISE as f32;

    fn source(kind: SourceKind, seed: i32, scale: i32, octaves: usize) -> Module {
        Module::source(Source::new(
            kind,
            SourceConfig {
                seed,
                octaves,
                frequency: 1.0 / scale as f32,
                ..SourceConfig::default()
            },
        ))
    }

    /// The oracle's noises, by index.
    fn noise(index: usize) -> Module {
        match index {
            0 => Module::constant(0.2),
            1 => Module::constant(0.55),
            2 => Module::constant(0.9),
            3 => Module::constant(0.0),
            4 => Module::constant(-0.25),
            5 => Module::constant(1.5),
            6 => source(SourceKind::Perlin, 7, 40, 2),
            7 => Module::constant(f32::NAN),
            8 => Module::constant(0.3),
            9 => source(SourceKind::Simplex, -3, 25, 3),
            10 => Module::constant(0.7),
            11 => Module::constant(f32::from_bits(BOUND.to_bits() - 1)),
            12 => Module::constant(BOUND),
            13 => Module::constant(f32::from_bits(BOUND.to_bits() + 1)),
            14 => Module::constant(-0.0),
            _ => unreachable!(),
        }
    }

    const TERRAINS: [&str; 8] = [
        "flats",
        "hills",
        "mountains",
        "plateau",
        "badlands",
        "none",
        "coast",
        "beach",
    ];

    fn terrain(
        registry: &TerrainRegistry,
        terrain: usize,
        weight: f32,
        noise_index: usize,
    ) -> Arc<TerrainNoise> {
        let terrain = registry.get_by_name(TERRAINS[terrain]).unwrap();
        Arc::new(TerrainNoise::new(terrain, weight, noise(noise_index)))
    }

    /// `{terrain, weight, noise}` per terrain noise, per set.
    const SETS: [&[(usize, f32, usize)]; 8] = [
        &[(0, 2.0, 0), (1, 3.0, 1), (2, 5.0, 2)],
        &[(3, 1.0, 3), (4, 1.0, 4), (5, 1.0, 5), (6, 2.0, 6)],
        &[(7, 1.0, 7), (0, 1.0, 8), (1, 1.0, 9), (2, 1.0, 10)],
        &[(0, 1.0, 11), (1, 1.0, 12), (2, 1.0, 13), (3, 1.0, 14)],
        &[(6, 1.0, 6)],
        &[(0, 0.0, 0), (1, 1.0, 9), (2, 3.0, 7)],
        &[(0, f32::NAN, 0), (1, 1.0, 1)],
        &[(1, 1.0, 0), (1, 2.0, 1), (2, 1.0, 2)],
    ];

    const SEEDS: [i64; 9] = [
        91,
        0,
        -1,
        0x1_0000_0005,
        i64::MAX,
        i32::MAX as i64,
        i64::MIN,
        -21_491_124,
        123_456_789_012,
    ];
    const SCALES: [i32; 9] = [64, 200, 32, 7, 1, 500, -50, 300, 3];
    const JITTERS: [f32; 9] = [0.8, 1.0, 0.5, 2.0, 0.0, -0.7, 0.9, 1.0, 1e6];
    const BLENDINGS: [f32; 9] = [0.65, 0.0, 1.0, 0.4, 0.9, f32::NAN, 0.5, 2.0, -0.5];
    const SET_OF: [usize; 9] = [0, 1, 2, 3, 4, 5, 2, 6, 1];

    const S: f32 = f32::from_bits(1);
    const SPECIAL_POINTS: [(f32, f32); 12] = [
        (0.0, 0.0),
        (-0.0, -0.0),
        (1e7, -1e7),
        (-3e9, 2.5),
        (1e20, -1e20),
        (f32::MAX, 1.0),
        (f32::INFINITY, 0.0),
        (f32::NAN, 5.0),
        (S, -S),
        (-1e-3, f32::NEG_INFINITY),
        (16_777_217.0, -16_777_216.0),
        (-123_456.78, 98_765.43),
    ];

    /// The oracle's points: the special ones, then 60 spread over about ±3400 x ±2100.
    fn points() -> Vec<(f32, f32)> {
        let mut points = SPECIAL_POINTS.to_vec();
        points.extend((0..60).map(|i: i32| {
            (
                ((i * 7919) % 2003 - 1001) as f32 * 3.37,
                ((i * 104_729) % 1999 - 999) as f32 * 2.11,
            )
        }));
        points
    }

    fn try_value(value: impl FnOnce() -> f32) -> Option<f32> {
        catch_unwind(AssertUnwindSafe(value)).ok()
    }

    /// The lines `TerrainBlenderOracle` prints (`.local/terraforged/oracles/terrain_blender/`).
    fn oracle_lines() -> String {
        let registry = TerrainRegistry::new();
        let mut out = String::new();
        for index in 0..15 {
            let noise = terrain(&registry, 0, 1.0, index);
            let noise = noise.noise();
            writeln!(
                out,
                "N {index} {} {} {} {}",
                h(noise.min_value()),
                h(noise.max_value()),
                h(noise.value(3.5, -7.25)),
                h(noise.value(-1000.5, 77.0)),
            )
            .unwrap();
        }
        let points = points();
        for s in 0..SEEDS.len() {
            let terrains = SETS[SET_OF[s]]
                .iter()
                .map(|&(t, weight, n)| terrain(&registry, t, weight, n))
                .collect();
            let blender =
                TerrainBlender::new(SEEDS[s], SCALES[s], JITTERS[s], BLENDINGS[s], terrains);
            writeln!(out, "B {s} {}", blender.region_seed).unwrap();
            let mut resource = blender.blender_resource();
            for (p, &(x, z)) in points.iter().enumerate() {
                let value = match try_value(|| blender.value_with(x, z, &mut resource)) {
                    Some(a) => {
                        let b = blender.value(x, z);
                        assert_eq!(a.to_bits(), b.to_bits(), "fresh and reused blenders agree");
                        h(a)
                    }
                    None => "throw".to_owned(),
                };
                let name = catch_unwind(AssertUnwindSafe(|| {
                    blender.terrain(&resource).name().to_owned()
                }))
                .unwrap_or_else(|_| "throw".to_owned());
                writeln!(
                    out,
                    "P {s} {p} {value} {} {} {name}",
                    resource.closest_index(),
                    resource.closest_index2(),
                )
                .unwrap();
            }
        }
        out
    }

    #[test]
    fn blenders_are_shareable_across_worldgen_threads() {
        fn shareable<T: Send + Sync>() {}
        fn movable<T: Send>() {}
        shareable::<TerrainBlender>();
        shareable::<TerrainNoise>();
        movable::<Blender>();
        fn owned<T: Send + 'static>(_: T) {}
        let registry = TerrainRegistry::new();
        let blender = TerrainBlender::new(1, 64, 0.8, 0.65, vec![terrain(&registry, 1, 1.0, 1)]);
        let hills = registry.get_by_name("hills").unwrap();
        let finder = blender.find_nearest(0.0, 0.0, 1, 2, &hills);
        drop(blender);
        owned(finder);
    }

    #[test]
    fn terrain_blender_matches_the_jar() {
        // Java throws on 113 lines; the port panics there (caught, printed as "throw").
        let out = oracle_lines();
        let lines: Vec<_> = out.lines().collect();
        assert_eq!(lines.len(), 672);
        assert_eq!(lines[3], "N 3 00000000 3f800000 3ca0a0a1 3ca0a0a1");
        assert_eq!(lines[11], "N 11 00000000 3f800000 3d20a0a0 3d20a0a0");
        assert_eq!(lines[12], "N 12 3ca0a0a1 3ca0a0a1 3ca0a0a1 3ca0a0a1");
        assert_eq!(lines[15], "B 0 21491215");
        assert_eq!(lines[16], "P 0 0 3eca581b 1 0 flats");
        assert_eq!(lines[18], "P 0 2 3f666666 7 4 mountains");
        assert_eq!(lines[20], "P 0 4 throw -1 -1 throw");
        assert_eq!(lines[380], "B 5 -2125992525");
        assert_eq!(fnv(&out), 0x14b1_fe45_32e6_a8ff);
    }

    /// The `withSeed` runs' seeds, one per blender of `SEEDS`.
    const RESEEDS: [i64; 9] = [
        7,
        -91,
        0x1_0000_0005,
        i64::MIN,
        i32::MIN as i64,
        -21_491_124,
        123_456_789_012,
        0,
        -1,
    ];

    /// A fresh resource's `getValue`, closest pair and terrain, as the oracle's `value` prints them.
    fn reseed_value(blender: &TerrainBlender, x: f32, z: f32) -> String {
        let mut resource = blender.blender_resource();
        let value =
            try_value(|| blender.value_with(x, z, &mut resource)).map_or("throw".to_owned(), h);
        let name = catch_unwind(AssertUnwindSafe(|| {
            blender.terrain(&resource).name().to_owned()
        }))
        .unwrap_or_else(|_| "throw".to_owned());
        let (a, b) = (resource.closest_index(), resource.closest_index2());
        format!("{value} {a} {b} {name}")
    }

    #[test]
    fn reseeded_blenders_match_the_jar() {
        // `TerrainBlenderOracle`'s R lines: each blender reseeded, its noises (raised again, as the jar's bias reads
        // back as 0) and its values. Java throws on 226 of them.
        let registry = TerrainRegistry::new();
        let mut out = String::new();
        for s in 0..SEEDS.len() {
            let terrains = SETS[SET_OF[s]]
                .iter()
                .map(|&(t, weight, n)| terrain(&registry, t, weight, n))
                .collect();
            let blender =
                TerrainBlender::new(SEEDS[s], SCALES[s], JITTERS[s], BLENDINGS[s], terrains);
            let before = reseed_value(&blender, 37.25, -91.5);
            let reseeded = blender.with_seed(RESEEDS[s]);
            assert_eq!(
                reseed_value(&blender, 37.25, -91.5),
                before,
                "withSeed leaves blender {s}"
            );
            writeln!(out, "R {s} {}", reseeded.region_seed).unwrap();
            for (n, noise) in reseeded.terrains.values().iter().enumerate() {
                let module = noise.noise();
                writeln!(
                    out,
                    "RN {s} {n} {} {} {} {} {} {}",
                    noise.terrain().name(),
                    h(noise.weight),
                    h(module.min_value()),
                    h(module.max_value()),
                    h(module.value(3.5, -7.25)),
                    h(module.value(-1000.5, 77.0)),
                )
                .unwrap();
            }
            for (p, &(x, z)) in points().iter().enumerate() {
                writeln!(out, "RP {s} {p} {}", reseed_value(&reseeded, x, z)).unwrap();
            }
        }
        let lines: Vec<_> = out.lines().collect();
        assert_eq!(lines.len(), 686);
        assert_eq!(lines[0], "R 0 21491131");
        assert_eq!(lines[4], "RP 0 0 3f666667 4 3 mountains");
        assert_eq!(lines[381], "R 5 0");
        assert_eq!(
            lines[383],
            "RN 5 1 hills 3f800000 00000000 3f800000 3eac10af 3e9f000e"
        );
        assert_eq!(out.matches("throw").count(), 226);
        assert_eq!(fnv(&out), 0x9b02_f798_48a1_d38d);
    }

    /// The search runs' start points.
    const SEARCH_POINTS: [(f32, f32); 13] = [
        (0.0, 0.0),
        (-0.0, -0.0),
        (37.25, -91.5),
        (-240.0, 118.75),
        (-1e5, -3e4),
        (1e7, -1e7),
        (3e9, 1.0),
        (1e20, -1e20),
        (f32::MAX, 1.0),
        (f32::INFINITY, 0.0),
        (f32::NAN, 5.0),
        (5.0, f32::NAN),
        (f32::NEG_INFINITY, f32::NEG_INFINITY),
    ];
    /// `{minRadius, maxRadius}`: ring 0 (empty), several rings, one ring, none, a negative start (Java divides by
    /// zero) and wrapping ring sizes.
    const RADII: [(i32, i32); 6] = [
        (0, 3),
        (1, 8),
        (4, 6),
        (0, 0),
        (-1, 3),
        (i32::MAX - 1, i32::MAX),
    ];

    /// The lines `TerrainBlenderOracle.search` prints: per blender (the nine above, then one over set 7, whose
    /// first two noises are both named "hills") and start point, the cells `iterator(x, z, 1, 2)` visits, then per
    /// query terrain and radii four finder steps (`hasNext`, then `next`'s packed floats) or "null"/"throw".
    fn search_lines() -> String {
        let registry = TerrainRegistry::new();
        // Same name as the registry's hills, another instance and id; a name differing only in case; a terrain in no set.
        let hills_copy = Terrain::new(99, "hills", TerrainCategory::Flatland);
        let hills_case = Terrain::new(98, "Hills", TerrainCategory::Flatland);
        let river = registry.get_by_name("river").unwrap();
        let mut out = String::new();
        for s in 0..=SEEDS.len() {
            let (config, set) = if s == SEEDS.len() {
                (0, 7)
            } else {
                (s, SET_OF[s])
            };
            let terrains = SETS[set]
                .iter()
                .map(|&(t, weight, n)| terrain(&registry, t, weight, n))
                .collect();
            let blender = TerrainBlender::new(
                SEEDS[config],
                SCALES[config],
                JITTERS[config],
                BLENDINGS[config],
                terrains,
            );
            let name = |index: usize| registry.get_by_name(TERRAINS[SETS[set][index].0]).unwrap();
            let queries = [
                name(0),
                name(SETS[set].len() - 1),
                hills_copy.clone(),
                hills_case.clone(),
                river.clone(),
            ];
            for (p, &(x, z)) in SEARCH_POINTS.iter().enumerate() {
                write!(out, "I {s} {p}").unwrap();
                for position in blender.iterator(x, z, 1, 2) {
                    write!(out, " {},{}", unpack_left(position), unpack_right(position)).unwrap();
                }
                out.push('\n');
                for (q, query) in queries.iter().enumerate() {
                    for (r, &(min, max)) in RADII.iter().enumerate() {
                        write!(out, "F {s} {p} {q} {r}").unwrap();
                        let Some(mut finder) = blender.find_nearest(x, z, min, max, query) else {
                            out.push_str(" null\n");
                            continue;
                        };
                        for _ in 0..4 {
                            let more = finder.has_next();
                            let Ok(position) = catch_unwind(AssertUnwindSafe(|| finder.next()))
                            else {
                                out.push_str(" throw");
                                break;
                            };
                            assert_eq!(
                                position.is_some(),
                                more,
                                "next is None exactly when has_next is false"
                            );
                            let position = position.unwrap_or(0);
                            write!(
                                out,
                                " {}:{},{}",
                                u8::from(more),
                                h(unpack_left_float(position)),
                                h(unpack_right_float(position)),
                            )
                            .unwrap();
                        }
                        out.push('\n');
                    }
                }
            }
        }
        out
    }

    #[test]
    fn nearest_terrain_search_matches_the_jar() {
        let out = search_lines();
        let lines: Vec<_> = out.lines().collect();
        assert_eq!(lines.len(), 4030);
        // Ring 1 then ring 2 around the warped cell, each from its (-r, -r) corner along +x, +z, -x, -z.
        assert_eq!(
            lines[0],
            "I 0 0 -1,-1 0,-1 1,-1 1,0 1,1 0,1 -1,1 -1,0 -2,-2 -1,-2 0,-2 1,-2 2,-2 2,-1 2,0 2,1 2,2 1,2 0,2 \
             -1,2 -2,2 -2,1 -2,0 -2,-1"
        );
        // Ring 0 is empty, so radii 0..3 and 1..8 find the same cells; each step resumes the walk.
        assert_eq!(
            lines[1],
            "F 0 0 0 0 1:41af898b,c2003568 1:42c66aa0,c2d9ddaa 1:4304a861,40c9a94d 1:42cb37c4,431dfa6d"
        );
        assert_eq!(lines[2][9..], lines[1][9..]);
        // No rings; a negative start radius divides by zero; a miss on the last ring returns Java's 0L / frequency.
        assert_eq!(
            lines[4],
            "F 0 0 0 3 0:00000000,00000000 0:00000000,00000000 0:00000000,00000000 0:00000000,00000000"
        );
        assert_eq!(lines[5], "F 0 0 0 4 throw");
        assert_eq!(
            lines[6],
            "F 0 0 0 5 1:00000000,00000000 0:00000000,00000000 0:00000000,00000000 0:00000000,00000000"
        );
        // A negative frequency (scale -50) turns that 0L into -0.0.
        assert!(out.contains(
            "\nF 6 0 0 5 1:80000000,80000000 0:00000000,00000000 0:00000000,00000000 0:00000000,00000000\n"
        ));
        // Names match with `String.equals`: another instance named "hills" finds the first "hills" noise's band.
        assert_eq!(lines[3702][9..], lines[3690][9..]);
        assert!(lines[3702].starts_with("F 9 2 2 0 1:4191054f,c32f21e7"));
        assert_eq!(lines.iter().filter(|l| l.ends_with(" null")).count(), 1794);
        assert_eq!(fnv(&out), 0xb76e_5dcb_6b68_055a);
    }
}
