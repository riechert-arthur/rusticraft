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

use std::sync::{Arc, RwLock};

use super::terraforged::{java_max, java_min, lerp, map, round};
use super::terraforged_engine::{
    Cell, ControlPoints, Levels, Terrain, TerrainProperties, TerrainRegistry, WorldControlPoints,
};

/// Engine's `FilterSettings.Smoothing` (defaults: one iteration, radius 1.8, rate 0.9). Engine's
/// `@Range` limits (iterations and radius 0 to 5, rate 0 to 1) are only enforced by its settings UI,
/// so values outside them are accepted here as in Engine.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SmoothingSettings {
    pub iterations: i32,
    pub smoothing_radius: f32,
    pub smoothing_rate: f32,
}

impl Default for SmoothingSettings {
    fn default() -> Self {
        Self {
            iterations: 1,
            smoothing_radius: 1.8,
            smoothing_rate: 0.9,
        }
    }
}

/// Engine's `FilterSettings.Erosion` (defaults: 135 droplets per chunk, lifetime 12, volume and
/// velocity 0.7, erosion and deposit rates 0.5; Engine's field is spelled `depositeRate`). As for
/// smoothing, Engine's `@Range` limits are only enforced by its settings UI.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ErosionSettings {
    pub droplets_per_chunk: i32,
    pub droplet_lifetime: i32,
    pub droplet_volume: f32,
    pub droplet_velocity: f32,
    pub erosion_rate: f32,
    pub deposit_rate: f32,
}

impl Default for ErosionSettings {
    fn default() -> Self {
        Self {
            droplets_per_chunk: 135,
            droplet_lifetime: 12,
            droplet_volume: 0.7,
            droplet_velocity: 0.7,
            erosion_rate: 0.5,
            deposit_rate: 0.5,
        }
    }
}

/// Engine's `Filterable`: a square map of `total_size() * total_size()` cells stored row by row
/// (`Size.indexOf(x, z) = z * total + x`). `cell_raw` is `getCellRaw`, which Engine's `Tile` answers
/// with `Cell.empty()` outside the backing array; here that is `None`.
pub trait Filterable {
    fn block_x(&self) -> i32;

    fn block_z(&self) -> i32;

    fn total_size(&self) -> usize;

    fn border_size(&self) -> usize;

    fn backing(&self) -> &[Cell];

    fn backing_mut(&mut self) -> &mut [Cell];

    fn cell_raw(&self, x: usize, z: usize) -> Option<&Cell> {
        let total = self.total_size();
        self.backing().get(z.wrapping_mul(total).wrapping_add(x))
    }

    fn cell_raw_mut(&mut self, x: usize, z: usize) -> Option<&mut Cell> {
        let total = self.total_size();
        self.backing_mut()
            .get_mut(z.wrapping_mul(total).wrapping_add(x))
    }
}

/// Engine's `filter.Modifier` as its filters build it: `Modifier.range(min, max)`, optionally
/// wrapped by `invert()`, with `Modifier.modify`'s strength adjustments.
#[derive(Clone, Copy, Debug)]
pub struct Modifier {
    min: f32,
    max: f32,
    range: f32,
    /// How many times `invert()` wrapped the range modifier: each one is `v -> 1 - inner(v)`.
    inversions: u32,
}

impl Modifier {
    /// `Modifier.range(min, max)`: 0 below `min`, 1 above `max`, linear in between.
    pub fn range(min: f32, max: f32) -> Self {
        Self {
            min,
            max,
            range: max - min,
            inversions: 0,
        }
    }

    /// `invert()`: one minus this modifier's value (applied again for each further `invert()`, as
    /// Engine's nested lambdas do; `1 - (1 - v)` need not round back to `v`).
    pub fn invert(mut self) -> Self {
        self.inversions += 1;
        self
    }

    /// `getValueModifier(value)`.
    fn value(self, value: f32) -> f32 {
        let mut modifier = if value > self.max {
            1.0
        } else if value < self.min {
            0.0
        } else {
            (value - self.min) / self.range
        };
        for _ in 0..self.inversions {
            modifier = 1.0 - modifier;
        }
        modifier
    }

    /// `Modifier.modify(cell, value)`: weakened near terrain-region edges of terrains whose erosion
    /// modifier is not 1, and near rivers.
    pub fn modify(self, cell: &Cell, value: f32) -> f32 {
        let mut strength = 1.0;
        let erosion = cell.terrain.erosion_modifier();
        if erosion != 1.0 {
            let alpha = map(cell.terrain_region_edge, 0.0, 0.15, 0.15);
            strength = lerp(1.0, erosion, alpha);
        }
        if cell.river_mask < 0.1 {
            strength *= map(cell.river_mask, 0.002, 0.1, 0.098);
        }
        self.value(cell.value) * strength * value
    }
}

/// Engine's `util.FastRandom` (a SplitMix-style stream), as `Erosion.apply` uses it: reseeded per
/// chunk with `seed(chunkSeed, iterationSeed)`, which mixes the second value into the gamma, then
/// drawn from with `nextInt(bound)`.
#[derive(Clone, Copy, Debug)]
pub struct FastRandom {
    seed: u64,
    gamma: u64,
}

impl FastRandom {
    /// `new FastRandom().seed(seed, gamma)`: the gamma goes through `mixGamma`.
    pub fn seeded(seed: i64, gamma: i64) -> Self {
        Self {
            seed: seed as u64,
            gamma: mix_gamma(gamma as u64),
        }
    }

    /// `nextInt(bound)`: masks for powers of two (and for 0 and `i32::MIN`, as in Engine), otherwise
    /// rejects draws whose `u + m - r` wraps negative. A non-positive bound is not rejected (Engine
    /// doesn't check it): 0 returns the raw draw, a negative bound a remainder of its magnitude.
    pub fn next_int(&mut self, bound: i32) -> i32 {
        let mut result = mix32(self.next_seed());
        let mask = bound.wrapping_sub(1);
        if bound & mask == 0 {
            return result & mask;
        }
        let mut unsigned = (result as u32 >> 1) as i32;
        loop {
            result = unsigned % bound;
            if unsigned.wrapping_add(mask).wrapping_sub(result) >= 0 {
                return result;
            }
            unsigned = (mix32(self.next_seed()) as u32 >> 1) as i32;
        }
    }

    fn next_seed(&mut self) -> u64 {
        self.seed = self.seed.wrapping_add(self.gamma);
        self.seed
    }
}

fn mix32(mut value: u64) -> i32 {
    value = (value ^ (value >> 33)).wrapping_mul(7_109_453_100_751_455_733);
    ((value ^ (value >> 28)).wrapping_mul((-3_808_689_974_395_783_757_i64) as u64) >> 32) as i32
}

fn mix_gamma(mut value: u64) -> u64 {
    value = (value ^ (value >> 33)).wrapping_mul((-49_064_778_989_728_563_i64) as u64);
    value = (value ^ (value >> 33)).wrapping_mul((-4_265_267_296_055_464_877_i64) as u64);
    value = (value ^ (value >> 33)) | 1;
    if (value ^ (value >> 1)).count_ones() < 24 {
        value ^ 0xaaaa_aaaa_aaaa_aaaa
    } else {
        value
    }
}

/// Engine's `Erosion.initBrushes(size, radius)` (`Erosion` uses radius 4): for each cell of a
/// `size * size` map, the indices of the cells within `radius` (squared distance below `radius²`,
/// clipped to the map) and their weights `1 - d / radius`, normalized to sum to 1. As in Engine,
/// only cells near the map's edge (rows up to `radius` from the top and from `size - radius`,
/// columns up to `radius + 1` and from `size - radius`) compute their own brush; every other cell
/// reuses the offsets and weights of the last computed one, re-centred. The brushes are stored in
/// one flat array (Engine allocates one pair of arrays per cell).
#[derive(Debug)]
pub struct ErosionBrushes {
    /// `starts[i]..starts[i + 1]` is cell `i`'s range in `indices` and `weights`.
    starts: Vec<usize>,
    indices: Vec<usize>,
    weights: Vec<f32>,
}

impl ErosionBrushes {
    /// Panics where Engine throws: a `size * size` or `radius * radius * 4` that is negative or
    /// does not fit Java's `int`, or more points in a brush than `radius * radius * 4` (only for
    /// radii whose square wraps).
    pub fn new(size: usize, radius: i32) -> Self {
        let cells = i32::try_from(size)
            .ok()
            .and_then(|size| size.checked_mul(size))
            .expect("Erosion's map size squared must fit Java's int");
        let size = size as i32;
        let capacity = usize::try_from(radius.wrapping_mul(radius).wrapping_mul(4))
            .expect("Erosion's brush capacity must not be negative");
        let mut x_offsets = vec![0_i32; capacity];
        let mut y_offsets = vec![0_i32; capacity];
        let mut weights = vec![0.0_f32; capacity];
        let mut weight_sum = 0.0_f32;
        let mut add_index = 0;
        let mut brushes = Self {
            starts: Vec::with_capacity(cells as usize + 1),
            indices: Vec::new(),
            weights: Vec::new(),
        };
        brushes.starts.push(0);
        let radius_squared = radius.wrapping_mul(radius) as f32;
        for index in 0..cells {
            let center_x = index % size;
            let center_y = index / size;
            if center_y <= radius
                || center_y >= size.wrapping_sub(radius)
                || center_x <= radius.wrapping_add(1)
                || center_x >= size.wrapping_sub(radius)
            {
                weight_sum = 0.0;
                add_index = 0;
                for y in radius.wrapping_neg()..=radius {
                    for x in radius.wrapping_neg()..=radius {
                        let squared_distance =
                            x.wrapping_mul(x).wrapping_add(y.wrapping_mul(y)) as f32;
                        // Engine's `!(sqrDst < radius²)`; neither side can be NaN.
                        if squared_distance >= radius_squared {
                            continue;
                        }
                        let coordinate_x = center_x.wrapping_add(x);
                        let coordinate_y = center_y.wrapping_add(y);
                        if coordinate_x < 0
                            || coordinate_x >= size
                            || coordinate_y < 0
                            || coordinate_y >= size
                        {
                            continue;
                        }
                        let weight =
                            1.0 - f64::from(squared_distance).sqrt() as f32 / radius as f32;
                        weight_sum += weight;
                        weights[add_index] = weight;
                        x_offsets[add_index] = x;
                        y_offsets[add_index] = y;
                        add_index += 1;
                    }
                }
            }
            for point in 0..add_index {
                let cell = y_offsets[point]
                    .wrapping_add(center_y)
                    .wrapping_mul(size)
                    .wrapping_add(x_offsets[point])
                    .wrapping_add(center_x);
                // Always inside the map: edge brushes are clipped and reused ones stay a full
                // radius away from every edge.
                brushes.indices.push(cell as usize);
                brushes.weights.push(weights[point] / weight_sum);
            }
            brushes.starts.push(brushes.indices.len());
        }
        brushes
    }

    /// `erosionBrushIndices[index]` and `erosionBrushWeights[index]`.
    pub fn get(&self, index: usize) -> (&[usize], &[f32]) {
        let range = self.starts[index]..self.starts[index + 1];
        (&self.indices[range.clone()], &self.weights[range])
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct TerrainPosition {
    height: f32,
    gradient_x: f32,
    gradient_y: f32,
}

impl TerrainPosition {
    fn at(&mut self, cells: &[Cell], map_size: usize, position_x: f32, position_y: f32) {
        let coordinate_x = position_x as usize;
        let coordinate_y = position_y as usize;
        let x = position_x - coordinate_x as f32;
        let y = position_y - coordinate_y as f32;
        let northwest = coordinate_y * map_size + coordinate_x;
        let height_northwest = cells[northwest].value;
        let height_northeast = cells[northwest + 1].value;
        let height_southwest = cells[northwest + map_size].value;
        let height_southeast = cells[northwest + map_size + 1].value;
        self.gradient_x = (height_northeast - height_northwest) * (1.0 - y)
            + (height_southeast - height_southwest) * y;
        self.gradient_y = (height_southwest - height_northwest) * (1.0 - x)
            + (height_southeast - height_northeast) * x;
        self.height = height_northwest * (1.0 - x) * (1.0 - y)
            + height_northeast * x * (1.0 - y)
            + height_southwest * (1.0 - x) * y
            + height_southeast * x * y;
    }
}

#[derive(Debug)]
pub struct Erosion {
    erode_speed: f32,
    deposit_speed: f32,
    initial_speed: f32,
    initial_water_volume: f32,
    max_droplet_lifetime: i32,
    brushes: ErosionBrushes,
    seed: i32,
    map_size: usize,
    modifier: Modifier,
}

impl Erosion {
    pub fn new(seed: i32, map_size: usize, settings: ErosionSettings, modifier: Modifier) -> Self {
        Self {
            erode_speed: settings.erosion_rate,
            deposit_speed: settings.deposit_rate,
            initial_speed: settings.droplet_velocity,
            initial_water_volume: settings.droplet_volume,
            max_droplet_lifetime: settings.droplet_lifetime,
            brushes: ErosionBrushes::new(map_size, 4),
            seed,
            map_size,
            modifier,
        }
    }

    pub fn factory(
        root_seed: i32,
        map_size: usize,
        settings: ErosionSettings,
        levels: Levels,
    ) -> Self {
        Self::new(
            root_seed.wrapping_add(12_768),
            map_size,
            settings,
            Modifier::range(levels.ground, levels.ground(15)),
        )
    }

    pub fn size(&self) -> usize {
        self.map_size
    }

    pub fn apply(
        &self,
        map: &mut impl Filterable,
        _region_x: i32,
        _region_z: i32,
        iterations: i32,
    ) {
        let chunk_x = map.block_x() >> 4;
        let chunk_z = map.block_z() >> 4;
        let length_chunks = map.total_size() >> 4;
        let border_chunks = map.border_size() >> 4;
        let map_size = map.total_size();
        let max_position = (map_size - 2) as f32;
        let cells = map.backing_mut();
        let mut first = TerrainPosition::default();
        let mut second = TerrainPosition::default();
        for iteration in 0..iterations {
            let iteration_seed = pack_seed(self.seed, iteration);
            for chunk_offset_z in 0..length_chunks {
                let relative_z = chunk_offset_z << 4;
                let seed_z = chunk_z + chunk_offset_z as i32 - border_chunks as i32;
                for chunk_offset_x in 0..length_chunks {
                    let relative_x = chunk_offset_x << 4;
                    let seed_x = chunk_x + chunk_offset_x as i32 - border_chunks as i32;
                    let chunk_seed = pack_seed(seed_x, seed_z);
                    let mut random = FastRandom::seeded(chunk_seed, iteration_seed);
                    let position_x =
                        (relative_x as f32 + random.next_int(16) as f32).clamp(1.0, max_position);
                    let position_y =
                        (relative_z as f32 + random.next_int(16) as f32).clamp(1.0, max_position);
                    self.apply_drop(position_x, position_y, cells, &mut first, &mut second);
                }
            }
        }
    }

    fn apply_drop(
        &self,
        mut position_x: f32,
        mut position_y: f32,
        cells: &mut [Cell],
        first: &mut TerrainPosition,
        second: &mut TerrainPosition,
    ) {
        let mut direction_x = 0.0_f32;
        let mut direction_y = 0.0_f32;
        let mut sediment = 0.0_f32;
        let mut speed = self.initial_speed;
        let mut water = self.initial_water_volume;
        *first = TerrainPosition::default();
        *second = TerrainPosition::default();
        for _ in 0..self.max_droplet_lifetime {
            let node_x = position_x as usize;
            let node_y = position_y as usize;
            let droplet_index = node_y * self.map_size + node_x;
            let cell_offset_x = position_x - node_x as f32;
            let cell_offset_y = position_y - node_y as f32;
            first.at(cells, self.map_size, position_x, position_y);
            direction_x = direction_x * 0.05 - first.gradient_x * 0.95;
            direction_y = direction_y * 0.05 - first.gradient_y * 0.95;
            let mut length =
                f64::from(direction_x * direction_x + direction_y * direction_y).sqrt() as f32;
            if length.is_nan() {
                length = 0.0;
            }
            if length != 0.0 {
                direction_x /= length;
                direction_y /= length;
            }
            position_x += direction_x;
            position_y += direction_y;
            if (direction_x == 0.0 && direction_y == 0.0)
                || position_x < 0.0
                || position_x >= (self.map_size - 1) as f32
                || position_y < 0.0
                || position_y >= (self.map_size - 1) as f32
            {
                return;
            }
            second.at(cells, self.map_size, position_x, position_y);
            let height_delta = second.height - first.height;
            let sediment_capacity = java_max(-height_delta * speed * water * 4.0, 0.01);
            if sediment > sediment_capacity || height_delta > 0.0 {
                let deposit = if height_delta > 0.0 {
                    java_min(height_delta, sediment)
                } else {
                    (sediment - sediment_capacity) * self.deposit_speed
                };
                sediment -= deposit;
                self.deposit(
                    &mut cells[droplet_index],
                    deposit * (1.0 - cell_offset_x) * (1.0 - cell_offset_y),
                );
                self.deposit(
                    &mut cells[droplet_index + 1],
                    deposit * cell_offset_x * (1.0 - cell_offset_y),
                );
                self.deposit(
                    &mut cells[droplet_index + self.map_size],
                    deposit * (1.0 - cell_offset_x) * cell_offset_y,
                );
                self.deposit(
                    &mut cells[droplet_index + self.map_size + 1],
                    deposit * cell_offset_x * cell_offset_y,
                );
            } else {
                let erosion = java_min(
                    (sediment_capacity - sediment) * self.erode_speed,
                    -height_delta,
                );
                let (indices, weights) = self.brushes.get(droplet_index);
                for (&index, &weight) in indices.iter().zip(weights) {
                    let amount = erosion * weight;
                    let sediment_delta = java_min(cells[index].value, amount);
                    self.erode(&mut cells[index], sediment_delta);
                    sediment += sediment_delta;
                }
            }
            speed = f64::from(speed * speed + height_delta * 3.0).sqrt() as f32;
            water *= 0.99;
            if speed.is_nan() {
                speed = 0.0;
            }
        }
    }

    fn deposit(&self, cell: &mut Cell, amount: f32) {
        if !cell.erosion_mask {
            let change = self.modifier.modify(cell, amount);
            cell.value += change;
            cell.sediment += change;
        }
    }

    fn erode(&self, cell: &mut Cell, amount: f32) {
        if !cell.erosion_mask {
            let change = self.modifier.modify(cell, amount);
            cell.value -= change;
            cell.erosion -= change;
        }
    }
}

fn pack_seed(x: i32, z: i32) -> i64 {
    (u64::from(x as u32) | (u64::from(z as u32) << 32)) as i64
}

/// Engine's `Smoothing` filter: pulls each cell's height towards the weighted mean of the present
/// cells within `smoothing_radius`, in place (later cells see earlier cells' new values), skipping
/// the outer `radius` cells of the map and cells under the erosion mask.
#[derive(Clone, Copy, Debug)]
pub struct Smoothing {
    radius: i32,
    radius_squared: f32,
    strength: f32,
    modifier: Modifier,
}

impl Smoothing {
    pub fn new(settings: SmoothingSettings, levels: Levels) -> Self {
        Self {
            radius: round(settings.smoothing_radius + 0.5),
            radius_squared: settings.smoothing_radius * settings.smoothing_radius,
            strength: settings.smoothing_rate,
            modifier: Modifier::range(levels.ground(1), levels.ground(120)).invert(),
        }
    }

    /// `apply(map, seedX, seedZ, iterations)`: `iterations` passes (none when it is not positive);
    /// the seeds are unused, as in Engine.
    pub fn apply(
        &self,
        map: &mut impl Filterable,
        _seed_x: i32,
        _seed_z: i32,
        mut iterations: i32,
    ) {
        while iterations > 0 {
            self.apply_once(map);
            iterations -= 1;
        }
    }

    fn apply_once(&self, map: &mut impl Filterable) {
        // A negative radius (a smoothing radius below -1) makes Engine's neighbourhood loops empty,
        // so no cell has any weight and nothing changes.
        let Ok(radius) = usize::try_from(self.radius) else {
            return;
        };
        let Some(max) = map.total_size().checked_sub(radius) else {
            return;
        };
        let reach = self.radius as isize;
        for z in radius..max {
            for x in radius..max {
                let cell = map
                    .cell_raw(x, z)
                    .expect("filter coordinates are in bounds");
                if cell.erosion_mask {
                    continue;
                }
                let mut total = 0.0_f32;
                let mut weights = 0.0_f32;
                for dz in -reach..=reach {
                    for dx in -reach..=reach {
                        let distance_squared = (dx * dx + dz * dz) as f32;
                        if distance_squared > self.radius_squared {
                            continue;
                        }
                        let Some(neighbour) =
                            map.cell_raw(x.wrapping_add_signed(dx), z.wrapping_add_signed(dz))
                        else {
                            continue;
                        };
                        if neighbour.is_absent() {
                            continue;
                        }
                        let weight = 1.0 - distance_squared / self.radius_squared;
                        total += neighbour.value * weight;
                        weights += weight;
                    }
                }
                if weights > 0.0 {
                    let difference = cell.value - total / weights;
                    let adjustment = self.modifier.modify(cell, difference * self.strength);
                    map.cell_raw_mut(x, z)
                        .expect("filter coordinates are in bounds")
                        .value -= adjustment;
                }
            }
        }
    }
}

/// Engine's `Steepness` filter: sets each cell's `gradient` from its height differences to the 15
/// cells at `(dx, dz) * radius` for `dx, dz` in `-1..=2` (Engine's neighbourhood is asymmetric),
/// with neighbours below sea level raised to it. Neighbours `Tile.getCellRaw` answers with
/// `Cell.empty()` (outside the backing array) or that are absent are skipped. Engine's unused
/// `maxBeachLevel` field is not kept.
#[derive(Clone, Copy, Debug)]
pub struct Steepness {
    radius: i32,
    scaler: f32,
    water_level: f32,
}

impl Steepness {
    /// `new Steepness(radius, scaler, levels)`; `WorldFilters` uses radius 1 and scaler 10.
    pub fn new(radius: i32, scaler: f32, levels: Levels) -> Self {
        Self {
            radius,
            scaler,
            water_level: levels.water,
        }
    }

    /// `apply(map, seedX, seedZ, iterations)`: one pass over every cell, row by row (Engine's
    /// `Filter.iterate`); the seeds and the iteration count are unused, as in Engine.
    pub fn apply(&self, map: &mut impl Filterable, _seed_x: i32, _seed_z: i32, _iterations: i32) {
        let total = map.total_size();
        let radius = self.radius as f32;
        for z in 0..total {
            for x in 0..total {
                let cell_value = map
                    .cell_raw(x, z)
                    .expect("filter coordinates are in bounds")
                    .value;
                let mut total_height_difference = 0.0_f32;
                for dz in -1_i32..=2 {
                    for dx in -1_i32..=2 {
                        if dx == 0 && dz == 0 {
                            continue;
                        }
                        // Java `int` arithmetic: `cx + dx * radius`, then `z * total + x`.
                        let neighbour_x = (x as i32).wrapping_add(dx.wrapping_mul(self.radius));
                        let neighbour_z = (z as i32).wrapping_add(dz.wrapping_mul(self.radius));
                        let Some(neighbour) = java_cell_raw(map, neighbour_x, neighbour_z) else {
                            continue;
                        };
                        if neighbour.is_absent() {
                            continue;
                        }
                        let height = java_max(neighbour.value, self.water_level);
                        total_height_difference += (cell_value - height).abs() / radius;
                    }
                }
                map.cell_raw_mut(x, z)
                    .expect("filter coordinates are in bounds")
                    .gradient = java_min(1.0, total_height_difference * self.scaler);
            }
        }
    }
}

/// `Tile.getCellRaw(x, z)` with Java `int` coordinates: the index `z * total + x` wraps like Java's,
/// and an index outside the backing array is `Cell.empty()` (`None`).
fn java_cell_raw(map: &impl Filterable, x: i32, z: i32) -> Option<&Cell> {
    let index = z.wrapping_mul(map.total_size() as i32).wrapping_add(x);
    usize::try_from(index)
        .ok()
        .and_then(|index| map.backing().get(index))
}

/// Engine's `BeachDetect` filter: turns a coast cell (`isCoast`: coast, beach, or a terrain
/// delegating to one) whose continent edge is below the beach control point into the world's
/// beach terrain when the squared height gradient across the cells 8 blocks to the east and west
/// and to the north and south is below 0.275. A neighbour that is absent or outside the map
/// (`Tile.getCellRaw`'s `Cell.empty()`) is replaced by the cell itself and shortens the distance
/// from 17 by 8. Engine's unused `grad2` field is not kept.
#[derive(Clone, Debug)]
pub struct BeachDetect {
    transition: ControlPoints,
    beach: Terrain,
}

impl BeachDetect {
    const RADIUS: i32 = 8;
    const DIAMETER: i32 = 17;

    /// `new BeachDetect(context)`: `points` is `context.settings.world.controlPoints` (validated by
    /// `ControlPoints`, which falls back to the defaults); `terrains` is the world's registry, whose
    /// built-in `beach` the filter assigns.
    pub fn new(points: WorldControlPoints, terrains: &TerrainRegistry) -> Self {
        Self {
            transition: ControlPoints::new(points),
            beach: terrains.get_by_id(4),
        }
    }

    /// `apply(map, seedX, seedZ, iterations)`: one pass over every cell, row by row (Engine's
    /// `Filter.iterate`); the seeds and the iteration count are unused, as in Engine.
    pub fn apply(&self, map: &mut impl Filterable, _seed_x: i32, _seed_z: i32, _iterations: i32) {
        let total = map.total_size();
        for z in 0..total {
            for x in 0..total {
                let cell = map
                    .cell_raw(x, z)
                    .expect("filter coordinates are in bounds");
                // `!(a && b)`, not `edge >= beach`: a NaN edge fails Engine's `<` test.
                if !(cell.terrain.is_coast() && cell.continent_edge < self.transition.beach) {
                    continue;
                }
                // Java `int` arithmetic: `dz - 8` and so on, then `z * total + x`.
                let (cx, cz) = (x as i32, z as i32);
                let north = java_cell_raw(map, cx, cz.wrapping_sub(Self::RADIUS));
                let south = java_cell_raw(map, cx, cz.wrapping_add(Self::RADIUS));
                let east = java_cell_raw(map, cx.wrapping_add(Self::RADIUS), cz);
                let west = java_cell_raw(map, cx.wrapping_sub(Self::RADIUS), cz);
                let gradient_x = Self::gradient(east, west, cell);
                let gradient_z = Self::gradient(north, south, cell);
                if gradient_x * gradient_x + gradient_z * gradient_z < 0.275 {
                    map.cell_raw_mut(x, z)
                        .expect("filter coordinates are in bounds")
                        .terrain = self.beach.clone();
                }
            }
        }
    }

    /// `grad(a, b, def)`.
    fn gradient(first: Option<&Cell>, second: Option<&Cell>, default: &Cell) -> f32 {
        let mut distance = Self::DIAMETER;
        let first = match first {
            Some(cell) if !cell.is_absent() => cell,
            _ => {
                distance -= Self::RADIUS;
                default
            }
        };
        let second = match second {
            Some(cell) if !cell.is_absent() => cell,
            _ => {
                distance -= Self::RADIUS;
                default
            }
        };
        (first.value - second.value) / distance as f32
    }
}

/// Engine's `WorldErosion<T>`: a lazily built value shared between threads, rebuilt by `factory`
/// whenever `validator` rejects it for the requested context. Engine's `StampedLock` (optimistic
/// read, then a read lock) is a `RwLock` here; the value is shared as an `Arc`.
pub struct WorldErosion<T, F, V> {
    value: RwLock<Option<Arc<T>>>,
    factory: F,
    validator: V,
}

impl<T, F, V> WorldErosion<T, F, V>
where
    F: Fn(i32) -> T,
    V: Fn(&T, i32) -> bool,
{
    pub fn new(factory: F, validator: V) -> Self {
        Self {
            value: RwLock::new(None),
            factory,
            validator,
        }
    }

    /// Engine's `get(ctx)`: returns the cached value when the validator accepts it for `context`;
    /// otherwise takes the write lock, checks again (another thread may have rebuilt it) and builds
    /// a new value with the factory, which replaces the cached one.
    pub fn get(&self, context: i32) -> Arc<T> {
        // Engine's readValue: the read lock only covers taking the value; it is validated outside.
        let current = self
            .value
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        if let Some(value) = current.filter(|value| (self.validator)(value, context)) {
            return value;
        }
        let mut slot = self
            .value
            .write()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(value) = slot
            .as_ref()
            .filter(|value| (self.validator)(value, context))
        {
            return Arc::clone(value);
        }
        let value = Arc::new((self.factory)(context));
        *slot = Some(Arc::clone(&value));
        value
    }
}

#[cfg(test)]
mod tests {
    use super::super::terraforged_engine::TerrainRegistry;
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct TestMap {
        total: usize,
        cells: Vec<Cell>,
    }

    impl Filterable for TestMap {
        fn block_x(&self) -> i32 {
            -32
        }

        fn block_z(&self) -> i32 {
            48
        }

        fn total_size(&self) -> usize {
            self.total
        }

        fn border_size(&self) -> usize {
            0
        }

        fn backing(&self) -> &[Cell] {
            &self.cells
        }

        fn backing_mut(&mut self) -> &mut [Cell] {
            &mut self.cells
        }
    }

    /// The cell map `SmoothingOracle.java` (`.local/terraforged/oracles/smoothing/`) builds for case
    /// `salt`: hashed values, river masks on both sides of 0.1 and 0.002, region edges, erosion
    /// masks, absent cells, and the none/badlands/hills/mountains terrains (badlands' erosion
    /// modifier is 0.3, the others' 1).
    fn oracle_cells(registry: &TerrainRegistry, total: usize, salt: i32) -> Vec<Cell> {
        let terrains = ["none", "badlands", "hills", "mountains"]
            .map(|name| registry.get_by_name(name).expect("built-in terrain"));
        (0..(total * total) as i32)
            .map(|index| {
                let mut h = index
                    .wrapping_mul(7919)
                    .wrapping_add(salt.wrapping_mul(104_729));
                h ^= ((h as u32) >> 13) as i32;
                h = h.wrapping_mul(0x5bd1_e995);
                h ^= ((h as u32) >> 15) as i32;
                let mut cell = if h & 31 == 3 {
                    Cell::empty(registry)
                } else {
                    Cell::new(registry)
                };
                cell.value = (((h as u32) >> 8) & 0xFFFF) as f32 / 40_000.0 - 0.2;
                cell.river_mask = match ((h as u32) >> 4) & 7 {
                    0 => 0.001,
                    1 => 0.05,
                    2 => 0.0999,
                    3 => 0.1,
                    _ => 1.0,
                };
                cell.terrain_region_edge = (((h as u32) >> 24) & 63) as f32 / 256.0;
                cell.erosion_mask = h & 63 == 17;
                cell.terrain = terrains[(((h as u32) >> 1) & 3) as usize].clone();
                cell
            })
            .collect()
    }

    /// Replays `SmoothingOracle.java`, which runs Engine's `Smoothing.apply` on the same 14x14 maps:
    /// default settings for one and three passes, wider and narrower radii (0.4 weighs only the
    /// cell itself, 0 gives NaN weights, -0.7 rounds to radius 0, -2.5 to a negative radius, 7 leaves
    /// no interior), other rates and levels, and zero or negative iterations. Each line folds every
    /// cell's value bits (`fold * 31 + bits`, from 17) and lists every 23rd cell's bits from cell 5.
    #[test]
    fn smoothing_matches_engine_oracle() {
        let registry = TerrainRegistry::new();
        let total = 14;
        let cases = [
            (1.8, 0.9, 1, 256, 63),
            (1.8, 0.9, 3, 256, 63),
            (3.2, 0.5, 2, 384, 127),
            (0.4, 1.0, 1, 256, 63),
            (5.0, 0.75, 1, 256, 63),
            (0.0, 0.9, 1, 256, 63),
            (-0.7, 0.9, 1, 256, 63),
            (-2.5, 0.9, 1, 256, 63),
            (7.0, 0.9, 1, 256, 63),
            (1.8, 0.9, 0, 256, 63),
            (1.8, 0.9, -3, 256, 63),
        ];
        let mut out = String::new();
        for (case, (radius, rate, iterations, height, sea_level)) in cases.into_iter().enumerate() {
            let settings = SmoothingSettings {
                iterations,
                smoothing_radius: radius,
                smoothing_rate: rate,
            };
            let mut map = TestMap {
                total,
                cells: oracle_cells(&registry, total, case as i32),
            };
            Smoothing::new(settings, Levels::new(height, sea_level))
                .apply(&mut map, 123, -456, iterations);
            let mut fold = 17_i64;
            let mut samples = String::new();
            for (index, cell) in map.cells.iter().enumerate() {
                let bits = cell.value.to_bits();
                fold = fold.wrapping_mul(31).wrapping_add(i64::from(bits as i32));
                if index % 23 == 5 {
                    samples += &format!(" {bits:08x}");
                }
            }
            out += &format!("{case} {fold:016x}{samples}\n");
        }
        assert_eq!(
            out,
            "0 55c4c81f1c2fe0d3 be3d70a4 3f93fb15 3fa9a9fc 3fb2fec5 3f179097 3f4f573f 3f721b09 3e16bb99 3f47b4a2\n\
             1 d9d2f379b0262870 3ec0c156 3f65652d 3f49e4f8 3f44a71e 3f34d35b 3f0be331 3f282e1d 3e993dda be244674\n\
             2 1b58a6970691d7eb 3f89ac71 3f9fedfa 3ea7212e 3f2dfd8b 3fa27bb3 3ee513c3 3d9288ce 3f6e2339 3ecde9e2\n\
             3 0ed0ffc6695dc5f4 3e7edfa5 3f01844d bdc624de 3faca305 3fa6786c 3e9f34d8 3fa3a511 3e8caf4e 3f8cfaac\n\
             4 7a690cc803d7c0e3 bd77b4a4 3e626e97 3eca339c 3e8b39c0 3dfb15b6 3c2ee630 3f39460b 3f67c9ef 3f593c37\n\
             5 78a2e5a0d654cab2 3fa31687 3fa1c6a8 3f118c7e 3f9d2bd3 be2b8bad 3cbf4880 3e3f6fd1 bdd6f007 3fa31b71\n\
             6 65f09fe0648f8a36 3f5cd845 3f17c01a bdceb1c5 3f3d3f7d 3f371759 3b7f9700 3fae0f90 3fa7eb85 3ecb6e30\n\
             7 c6bb5f58b1771378 3fb684b5 3ee0f5c2 3f0d3c36 3f953b64 3f9400d1 3e6a9931 3f9fce07 3e204ea5 3dbf2e4a\n\
             8 ec8ecb93e7989f9e 3f3d54ca be3b8bad 3f78779b 3ea53f7e 3e14ea4b 3f4d38ef 3f843d70 3f46ca58 3f243c9f\n\
             9 9669758e74e9a8c9 3fa185f0 3f7507c9 3e22e48f 3eb47e28 bde538f0 bda5bc02 3f527a0f 3fa0d35a be0793de\n\
             10 e596f2904148f08e 3eac9ba6 3d56bb94 3f90a4a8 3f48538f 3fa786c2 3e932ca6 3fb6acda bd59999c 3f262b6b\n"
        );
    }

    /// Replays `SteepnessOracle.java` (`.local/terraforged/oracles/steepness/`), which runs Engine's
    /// `Steepness.apply` on the smoothing oracle's 14x14 maps: `WorldFilters`' radius 1 and scaler 10,
    /// wider radii, radius 0 (division by zero), negative radii, radii that reach past the map (7, 20)
    /// or wrap Java's `int` arithmetic (`i32::MAX`, `i32::MIN`), a negative scaler, swapped levels,
    /// and maps with NaN, -0.0 and 0.0 heights (one with water level 0). Each line folds every cell's
    /// gradient bits (NaNs canonical, `fold * 31 + bits`, from 17) and lists every 23rd cell's bits
    /// from cell 5.
    #[test]
    fn steepness_matches_engine_oracle() {
        let registry = TerrainRegistry::new();
        let total = 14;
        let cases = [
            (1, 10.0, 256, 63, false),
            (2, 0.05, 256, 63, false),
            (3, 0.5, 384, 127, false),
            (0, 10.0, 256, 63, false),
            (-1, 10.0, 256, 63, false),
            (-2, 2.5, 256, 63, false),
            (7, 0.1, 256, 63, false),
            (20, 10.0, 256, 63, false),
            (i32::MAX, 10.0, 256, 63, false),
            (i32::MIN, 10.0, 256, 63, false),
            (1, -3.0, 256, 63, false),
            (1, 0.2, 63, 255, false),
            (1, 0.25, 256, 0, true),
            (2, 1.0, 256, 63, true),
        ];
        let mut out = String::new();
        for (case, (radius, scaler, height, sea_level, specials)) in cases.into_iter().enumerate() {
            let mut cells = oracle_cells(&registry, total, case as i32);
            if specials {
                for (index, cell) in cells.iter_mut().enumerate() {
                    if index % 61 == 4 {
                        cell.value = f32::NAN;
                    }
                    if index % 13 == 6 {
                        cell.value = -0.0;
                    }
                    if index % 17 == 2 {
                        cell.value = 0.0;
                    }
                }
            }
            let mut map = TestMap { total, cells };
            Steepness::new(radius, scaler, Levels::new(height, sea_level))
                .apply(&mut map, 123, -456, 99);
            let mut fold = 17_i64;
            let mut samples = String::new();
            for (index, cell) in map.cells.iter().enumerate() {
                let bits = if cell.gradient.is_nan() {
                    0x7fc0_0000
                } else {
                    cell.gradient.to_bits()
                };
                fold = fold.wrapping_mul(31).wrapping_add(i64::from(bits as i32));
                if index % 23 == 5 {
                    samples += &format!(" {bits:08x}");
                }
            }
            out += &format!("{case} {fold:016x}{samples}\n");
        }
        assert_eq!(
            out,
            "0 dd8f5852f6207791 3f800000 3f800000 3f800000 3f800000 3f800000 3f800000 3f800000 3f800000 3f800000\n\
             1 2bdb47d06ebd99a0 3d5ee97a 3e19c0eb 3e02e6da 3de711f0 3dfe6668 3e8b4b5e 3e004799 3dbf28b7 3e16ddec\n\
             2 148098a72e7358f3 3f2c73c8 3f800000 3f800000 3f46bcae 3f800000 3f800000 3f800000 3ed9fcff 3e376087\n\
             3 1401a87416207791 7fc00000 7fc00000 3f800000 7fc00000 7fc00000 7fc00000 00000000 7fc00000 7fc00000\n\
             4 ce02f2cc83d7ab23 c257b688 c22275e3 c2884c28 c2464561 c2aa0cad c2c74354 c243f312 c2806b95 c248347e\n\
             5 af24849e597f53e0 c095e687 c09307af c0aa7ba4 c13bf924 c19f863e c13edd82 c0a13668 c162bee9 c0c42be6\n\
             6 6d08bffe7aae90a8 3d218847 3d1a15bf 3dc4aad1 3d40c34c 3d0fb18b 3dd2de02 3d6f11a2 3d761075 3cd3f410\n\
             7 4143b0a1239d946b 3f5bf764 3f2b1eb8 3f0dc01a 3f800000 3eb83fe4 3f1d7176 3f800000 3f76e978 3ebe1e50\n\
             8 f97440a2a3db3598 31be5a1c 32a63666 32a5ebc7 3263cc8a 32919f1b 3265be34 32596a7b 3229c7ae 31d80625\n\
             9 e9452892e248b625 00000000 00000000 b13a2c06 00000000 b2464d91 00000000 00000000 00000000 b251c4dd\n\
             10 e2a736b305a93819 c18e8824 c1a4ba6d c1a41fa9 c1896950 c1f1f8f6 c1ca3659 c1cff4ea c1c3354e c1142c98\n\
             11 068e0d0e17e14776 3f57be72 3f800000 3f800000 3f4f6112 3f800000 3f5bdaf3 3f800000 3f3b0e7d 3f2b2766\n\
             12 2b14e288d61df853 7fc00000 3f800000 7fc00000 3f800000 7fc00000 3f800000 3f800000 3f800000 3ed315b5\n\
             13 d16a5f5730b00644 7fc00000 7fc00000 3f800000 3f800000 3f800000 3f800000 3f800000 3f800000 7fc00000\n"
        );
    }

    /// `WorldFilters`' radius 1 and scaler 10 on a 6x6 map of heights just above sea level, with no
    /// absent cells: edge, corner and interior cells against Engine's gradient bits.
    #[test]
    fn steepness_matches_engine_0_3_0() {
        let registry = TerrainRegistry::new();
        let total = 6;
        let cells = (0..total * total)
            .map(|index| {
                let mut cell = Cell::new(&registry);
                cell.value = 0.25 + ((index * 29 + 7) % 97) as f32 / 32_768.0;
                cell
            })
            .collect();
        let mut map = TestMap { total, cells };

        Steepness::new(1, 10.0, Levels::new(256, 63)).apply(&mut map, 123, -456, 99);

        let expected = [
            (0, 0x3e0c0000),
            (1, 0x3dd7a000),
            (7, 0x3e3a9000),
            (14, 0x3e165000),
            (21, 0x3e0a2000),
            (28, 0x3da96000),
            (35, 0x3d1c4000),
        ];
        for (index, bits) in expected {
            assert_eq!(map.cells[index].gradient.to_bits(), bits, "cell {index}");
        }
    }

    /// Replays `BeachDetectOracle.java` (`.local/terraforged/oracles/beach/`), which runs Engine's
    /// `BeachDetect.apply` on hashed maps of coast, beach and other terrains with absent cells,
    /// NaN, infinite and signed-zero heights, NaN continent edges, maps smaller than the radius and
    /// valid and invalid beach control points. Per case: a fold of every cell's terrain id, the
    /// number of beach cells, the number of cells turned into beach and the first eight of them.
    #[test]
    fn beach_detection_matches_engine_oracle() {
        let registry = TerrainRegistry::new();
        // COAST, COAST, COAST, BEACH, NONE, SHALLOW_OCEAN, FLATS, RIVER
        let terrains = [3, 3, 3, 4, 0, 2, 8, 5].map(|id| registry.get_by_id(id));
        let beach = registry.get_by_id(4);
        let cases = [
            (20, 1.0_f32, false, 0.327_f32),
            (20, 4.0, false, 0.327),
            (20, 12.0, false, 0.327),
            (20, 30.0, true, 0.327),
            (17, 9.0, false, 0.4),
            (9, 9.0, false, 0.327),
            (8, 6.0, true, 0.327),
            (24, 9.0, true, 0.3),
            (16, 9.0, false, 0.9),
            (33, 20.0, true, 0.44),
        ];
        let mut out = String::new();
        for (case, (total, scale, specials, beach_point)) in cases.into_iter().enumerate() {
            let cells = (0..total * total)
                .map(|index| {
                    let mut h = (index as i32)
                        .wrapping_mul(7919)
                        .wrapping_add((case as i32).wrapping_mul(104_729));
                    h ^= ((h as u32) >> 13) as i32;
                    h = h.wrapping_mul(0x5bd1_e995);
                    h ^= ((h as u32) >> 15) as i32;
                    let mut cell = if h & 31 == 3 {
                        Cell::empty(&registry)
                    } else {
                        Cell::new(&registry)
                    };
                    let bits = h as u32;
                    cell.value = ((bits >> 8) & 0xffff) as f32 / 65_536.0 * scale - scale * 0.5;
                    cell.terrain = terrains[((bits >> 24) & 7) as usize].clone();
                    cell.continent_edge = ((bits >> 4) & 0x3ff) as f32 / 1_600.0;
                    if specials && index % 37 == 4 {
                        cell.value = f32::NAN;
                    }
                    if specials && index % 29 == 7 {
                        cell.continent_edge = f32::NAN;
                    }
                    if specials && index % 13 == 6 {
                        cell.value = -0.0;
                    }
                    if specials && index % 11 == 2 {
                        cell.value = f32::INFINITY;
                    }
                    cell
                })
                .collect::<Vec<_>>();
            let before = cells
                .iter()
                .map(|cell| cell.terrain.is_same(&beach))
                .collect::<Vec<_>>();
            let mut map = TestMap { total, cells };
            let points = WorldControlPoints {
                beach: beach_point,
                ..WorldControlPoints::default()
            };
            BeachDetect::new(points, &registry).apply(&mut map, 123, -456, 99);
            let mut fold = 17_i64;
            let mut beaches = 0;
            let mut turned = Vec::new();
            for (index, cell) in map.cells.iter().enumerate() {
                fold = fold
                    .wrapping_mul(31)
                    .wrapping_add(i64::from(cell.terrain.id()));
                if cell.terrain.is_same(&beach) {
                    beaches += 1;
                    if !before[index] {
                        turned.push(index);
                    }
                }
            }
            let first = turned.iter().take(8).map(|index| format!(" {index}"));
            out += &format!(
                "{case} {fold:016x} {beaches} {}{}\n",
                turned.len(),
                first.collect::<String>()
            );
        }
        assert_eq!(
            out,
            "0 a16232084b282657 129 80 0 14 15 17 24 33 48 50\n\
             1 523cf84637a15ed8 135 87 2 4 5 8 16 19 21 23\n\
             2 698133c6de0a2998 100 53 9 12 15 25 32 34 35 37\n\
             3 24a27b1ea817910c 51 5 28 284 309 320 367\n\
             4 c2298533b0ec2087 83 42 11 13 21 40 41 45 47 54\n\
             5 e3a2b4be1ab85643 22 11 7 20 27 29 44 50 51 53\n\
             6 c0b62d4973d97357 12 4 28 29 31 34\n\
             7 67a94bc7beee2b79 111 44 14 37 77 84 110 119 127 161\n\
             8 0fb25686d56c8179 67 38 0 12 13 15 32 49 53 56\n\
             9 1ba7e55f2c89924d 213 58 20 34 51 67 88 114 136 140\n"
        );
    }

    /// The filter assigns the registry's own beach (`is_same`), so later identity checks see it.
    #[test]
    fn beach_detection_assigns_the_world_beach() {
        let registry = TerrainRegistry::new();
        let total = 3;
        let mut cells = (0..total * total)
            .map(|_| Cell::new(&registry))
            .collect::<Vec<_>>();
        cells[4].terrain = registry.get_by_id(3);
        let mut map = TestMap { total, cells };
        BeachDetect::new(WorldControlPoints::default(), &registry).apply(&mut map, 0, 0, 0);
        assert!(map.cells[4].terrain.is_same(&registry.get_by_id(4)));
        assert!(
            !map.cells[4]
                .terrain
                .is_same(&TerrainRegistry::new().get_by_id(4))
        );
        assert!(map.cells[0].terrain.is_same(&registry.get_by_id(0)));
    }

    /// Engine's erosion defaults, eight `nextInt(16)` draws as `Erosion.apply` makes them for chunk
    /// (-11, 29) of iteration 7 at root seed 12345, and sampled points of corner, edge and interior
    /// brushes of a 12x12 map.
    #[test]
    fn erosion_primitives_match_engine_0_3_0() {
        assert_eq!(
            ErosionSettings::default(),
            ErosionSettings {
                droplets_per_chunk: 135,
                droplet_lifetime: 12,
                droplet_volume: 0.7,
                droplet_velocity: 0.7,
                erosion_rate: 0.5,
                deposit_rate: 0.5,
            }
        );
        let iteration_seed = pack_seed(12_768 + 12_345, 7);
        let chunk_seed = pack_seed(-11, 29);
        let mut random = FastRandom::seeded(chunk_seed, iteration_seed);
        assert_eq!(
            std::array::from_fn::<_, 8, _>(|_| random.next_int(16)),
            [14, 4, 10, 14, 11, 11, 8, 4]
        );

        let brushes = ErosionBrushes::new(12, 4);
        for (cell, expected) in [
            (0, [(0, 0x3e1f1215), (15, 0x3d0541cc), (38, 0x3c7afb1a)]),
            (65, [(27, 0x3bc0eb7f), (65, 0x3d748b2f), (103, 0x3bc0eb7f)]),
            (78, [(40, 0x3bc0eb7f), (78, 0x3d748b2f), (116, 0x3bc0eb7f)]),
            (
                143,
                [(105, 0x3c7afb1b), (128, 0x3d0541cd), (143, 0x3e1f1216)],
            ),
        ] {
            let (indices, weights) = brushes.get(cell);
            let points = [0, indices.len() / 2, indices.len() - 1];
            assert_eq!(
                points.map(|point| (indices[point], weights[point].to_bits())),
                expected,
                "brush {cell}"
            );
        }
    }

    /// Replays `ErosionPrimitivesOracle.java` (`.local/terraforged/oracles/erosion_primitives/`).
    /// `r` lines: Engine's `FastRandom` seeded as `Erosion.apply` seeds it (`NoiseUtil.seed` of a
    /// chunk and of `root + 12768` (wrapping) and an iteration), 12 `nextInt` draws for each bound
    /// (powers of two, 0, 1, negative, `i32::MIN`/`MAX` and bounds that make it reject draws): the
    /// first draw and a fold of all of them. `b` lines: `Erosion.initBrushes(size, radius)` for map
    /// sizes 0 to 40 and radii -1 to 6: the number of brush points and a fold of every cell's
    /// length, indices and weight bits.
    #[test]
    fn erosion_primitives_match_engine_oracle() {
        let mut out = String::new();
        for root in [12_345, -7, i32::MAX] {
            for iteration in 0..3 {
                let iteration_seed = pack_seed(root.wrapping_add(12_768), iteration);
                for (x, z) in [(-11, 29), (0, 0), (i32::MIN, i32::MAX)] {
                    out += &format!("r {root} {iteration} {x} {z}");
                    for bound in [
                        16,
                        1,
                        0,
                        3,
                        7,
                        100,
                        -1,
                        -3,
                        -16,
                        i32::MIN,
                        i32::MAX,
                        1_500_000_000,
                        0x4000_0001,
                    ] {
                        let mut random = FastRandom::seeded(pack_seed(x, z), iteration_seed);
                        let draws: [i32; 12] = std::array::from_fn(|_| random.next_int(bound));
                        let fold = draws.iter().fold(17_i64, |fold, &draw| {
                            fold.wrapping_mul(31).wrapping_add(i64::from(draw))
                        });
                        out += &format!(" {}:{fold:x}", draws[0]);
                    }
                    out += "\n";
                }
            }
        }
        assert_eq!(
            out,
            "r 12345 0 -11 29 12:be38420ac4fc6344 0:b9d3c92fd6b36e91 703628972:558c6501c3689d64 1:ba30f78911c6f42d 2:ba9a2e2a00831111 86:d847f4392843bb6b 0:b9d3c92fd6b36e91 1:ba30f78911c6f42d 6:bc06ba2bcf004f5b 703628972:a3f726b343689d64 351814486:561accc437947d7b 351814486:20cd767eb4a8947e 351814486:20c93b30a8aa55aa\n\
             r 12345 0 0 0 5:bb9d3cd9493eb76f 0:b9d3c92fd6b36e91 -254163931:827aea12ac3f51af 1:ba31111bc6c62db2 5:bba5e292725b5361 82:d6f7ea9adf2557c1 0:b9d3c92fd6b36e91 1:ba31111bc6c62db2 2:ba8c15230d5008d1 1893319717:2f8769d2ac3f51af 2020401682:cb06aaeb282e4901 1152695168:fc9361441635919 331681005:dd7fac17bd41cb0a\n\
             r 12345 0 -2147483648 2147483647 3:baf7f654da802d2d 0:b9d3c92fd6b36e91 -1639369581:1450fc002e4cdced 0:b9d3cabd6ad9cab8 0:b9e575e5fdb4b195 57:ce098c7f5c2164df 0:b9d3c92fd6b36e91 0:b9d3cabd6ad9cab8 9:bd0a35c71be24b9f 508114067:b106e9c1ae4cdced 1327798857:399b8c9b621f18f 1327798857:2526cf99bb5ccdab 245606685:cbd59e2c7ea82c64\n\
             r 12345 1 -11 29 4:bb63973c0e62754e 0:b9d3c92fd6b36e91 274292084:10d5ffedb4711afe 1:ba33e1bb9ff93d14 5:bba642b60d7bcf11 42:c92204f912bad20c 0:b9d3c92fd6b36e91 1:ba33e1bb9ff93d14 10:bd6c5c46b5ecc5b8 274292084:d461920db4711afe 137146042:28def589020df888 137146042:25d77a104bb807a1 137146042:b03747d7b7c7f292\n\
             r 12345 1 0 0 1:ba4f117cccd859f4 0:b9d3c92fd6b36e91 2013648929:bc2d5dc546c82114 2:ba8e55f3cbe39853 2:ba91282107a53b51 64:d105e11c90e65433 0:b9d3c92fd6b36e91 2:ba8e55f3cbe39853 0:b9fa28280cae4803 2013648929:3b21dfd5c6c82114 1006824464:39c67e5e8c7af983 1006824464:d3d1118e538dc3f1 1006824464:42b1d21522860379\n\
             r 12345 1 -2147483648 2147483647 14:bec72592e3d75f99 0:b9d3c92fd6b36e91 -141673794:b7a97e2ed5398359 2:ba8b547b73ee5ecc 4:bb48fb9014bf30ee 51:cbe4b7de9cc5d396 0:b9d3c92fd6b36e91 2:ba8b547b73ee5ecc 15:bf1ede01f7b00596 2005809854:7f069eeed5398359 2076646751:1a43b01084c086 1388299099:e78bcc7c389c3539 121538803:a3ef78e0d2017f17\n\
             r 12345 2 -11 29 1:ba4bdf22387cafa4 0:b9d3c92fd6b36e91 353176529:229068bf7bc1f3d4 2:ba8b6d4dfc89de6c 6:bbf7b27f08fe4765 64:d0d34d117d811328 0:b9d3c92fd6b36e91 2:ba8b6d4dfc89de6c 8:bcb58ce15e6b6b54 353176529:eb1cd7e07bc1f3d4 176588264:3691593c2c4faf94 176588264:26ba59130cd2e94a 176588264:ec53aa34de877e8d\n\
             r 12345 2 0 0 5:bba9ecb6968e53a3 0:b9d3c92fd6b36e91 2054166037:8c1feb2392954933 0:b9d3e1489e460a2e 3:baf48d70bb06c2dd 18:c12fe825ccc5bd6e 0:b9d3c92fd6b36e91 0:b9d3e1489e460a2e 10:bd64a64c9b73db62 2054166037:1ed1d53412954933 1027083018:357ea1ca30cb8932 1027083018:1b27f13b3b12301b 1027083018:cd75d422ff69a06\n\
             r 12345 2 -2147483648 2147483647 11:bddae67cfe94bab9 0:b9d3c92fd6b36e91 1180869643:e7608704eb925549 0:b9d9cc39ba9507b1 4:bb3d272e6118393a 21:c15af06a68c95d14 0:b9d3c92fd6b36e91 0:b9d9cc39ba9507b1 5:bbc0003baee35eac 1180869643:2c81e3646b925549 590434821:958ce14a7e38d3ec 590434821:2002c79a5d1cbb06 590434821:4945e0ab21522e57\n\
             r -7 0 -11 29 12:be38134210f9145b 0:b9d3c92fd6b36e91 -1234656980:29f8681777040ccb 1:ba2e0e167b492334 0:b9d45b04bfd8c9b1 58:cf6bbee87bca5303 0:b9d3c92fd6b36e91 1:ba2e0e167b492334 6:bc05e232ded822cf 912826668:5cc83df5f7040ccb 1530155158:a4b5e27c11693f0f 97688199:66fe9c7daf48ee61 97688199:b7019821998a2bb2\n\
             r -7 0 0 0 14:bedb69a1a71c7faa 0:b9d3c92fd6b36e91 -1413288514:8de206f90da77b7a 0:b9d3e2d5fbb52333 5:bba5e35892a29054 91:da44d7c8e5099bb2 0:b9d3c92fd6b36e91 0:b9d3e2d5fbb52333 15:bf2a80f8b6a97556 734195134:3bc1e7488da77b7a 1440839391:51bac860bd627066 1440839391:3a1444092d0647ce 526798382:ca95899d9cce415e\n\
             r -7 0 -2147483648 2147483647 6:bbf8611b2c723804 0:b9d3c92fd6b36e91 -936119434:ae9123ef8335e594 0:b9d6cc4859e28bcc 3:baebcc89e667f62b 31:c58f6174677af3a3 0:b9d3c92fd6b36e91 0:b9d6cc4859e28bcc 11:bdb841f59798aa43 1211364214:5b9da5908335e594 1679423931:613ef7ca2e006403 1402675846:6e6cc47faa9a9c5f 606750500:e06afe870267796c\n\
             r -7 1 -11 29 5:bb9a2df7e10170e0 0:b9d3c92fd6b36e91 -734086859:5deaf5c57246af40 2:ba8b6e14c91eb590 4:bb459aa2ed5f22dc 18:c06b3c30fc46c396 0:b9d3c92fd6b36e91 2:ba8b6e14c91eb590 10:bd5b3ff7b1e6b7aa 1413396789:e10832357246af40 1780440218:8ecdf8bb5b32bdea 1466423920:19b028ff5ff3b42d 849536744:f858dae709edb088\n\
             r -7 1 0 0 3:bafa7d4891abad9b 0:b9d3c92fd6b36e91 152890739:8f63f36d5398babb 2:ba8884956469918f 0:b9d7152c69f66cc9 69:d3484ecdbecdc2f6 0:b9d3c92fd6b36e91 2:ba8884956469918f 9:bd237362bc37d276 152890739:a403116dd398babb 76445369:390dd9df1145fc06 76445369:b8b74665da4a0bca 76445369:ad3a911a2d83511b\n\
             r -7 1 -2147483648 2147483647 1:ba3cd46739fee7ba 0:b9d3c92fd6b36e91 1533274529:edeed0593c3796a 2:ba8e3f7afe4643f0 1:ba3fd2c564285ea3 64:d0b1afdc54273841 0:b9d3c92fd6b36e91 2:ba8e3f7afe4643f0 0:b9da7dd1b5b3521d 1533274529:d263755513c3796a 766637264:27af4bda68dd3ded 766637264:53132795ed79635e 766637264:f530c37b02145187\n\
             r -7 2 -11 29 7:bc6658ccc83a1bf5 0:b9d3c92fd6b36e91 -508185593:a0583a743baf15d5 2:ba8884a1ca5f710a 2:ba91140f99eb8152 51:cc4bd642d0484b90 0:b9d3c92fd6b36e91 2:ba8884a1ca5f710a 3:bb0679c1c348a444 1639298055:f294c1433baf15d5 1893390851:7f23e56e8bef4d24 467548140:ed25b4bf0b213c82 467548140:6f8951676af68b37\n\
             r -7 2 0 0 9:bd0382d5d639b020 0:b9d3c92fd6b36e91 -816281767:190fe1bed7e65a90 1:ba33f9ce642ddd55 5:bba05a1275a6f69c 64:d16837454e0342eb 0:b9d3c92fd6b36e91 1:ba33f9ce642ddd55 12:be0f3bde41df792f 1331201881:683c85ded7e65a90 1739342764:386fe20aa38ae95f 331587730:b3dd93652e459b8e 331587730:b3dd93652e459b8e\n\
             r -7 2 -2147483648 2147483647 10:bd5eb99b52e5deed 0:b9d3c92fd6b36e91 -292705638:3b5fc74f7368776d 2:ba8e27690d8e68f4 5:bba5e34cd3da680f 29:c4e7802d852cfc07 0:b9d3c92fd6b36e91 2:ba8e27690d8e68f4 13:be81454284f1347f 1854778010:84b125dff368776d 2001130829:43e9b1b0413b41af 1136896472:1c30d4ed11b8080b 73798427:8c1af404048a7e92\n\
             r 2147483647 0 -11 29 13:be81b81d9eedb3e0 0:b9d3c92fd6b36e91 1398028173:3753e089a03f4d90 1:ba3110623729dbaf 1:ba36cf2f53251cda 86:d91168868a4217bd 0:b9d3c92fd6b36e91 1:ba3110623729dbaf 6:bc15b05fafc92f31 1398028173:1552561aa03f4d90 699014086:d66527fdb6c5e301 699014086:c5f7197de368ca2e 699014086:b7c507acfed2a264\n\
             r 2147483647 0 0 0 13:be89a23235f52af5 0:b9d3c92fd6b36e91 553533309:1e378785bf073835 1:ba2e3ef4c6a3e6cd 3:bae2ca6241e786e8 54:ccfd65a56d4d6ddb 0:b9d3c92fd6b36e91 1:ba2e3ef4c6a3e6cd 14:bee98b801e9a2783 553533309:6c9b3d85bf073835 276766654:ba3abb2b67fa1513 276766654:ee69f61acaffad8a 276766654:ee66189e4672bdf9\n\
             r 2147483647 0 -2147483648 2147483647 11:bdc39679e5b5ffc5 0:b9d3c92fd6b36e91 575283611:62d8ce3c181a32f5 2:ba8e5787275a358d 3:baee725a1107201f 5:bc99fe0117059744 0:b9d3c92fd6b36e91 2:ba8e5787275a358d 13:be867f6a93a0f754 575283611:c898b36d181a32f5 287641805:73e78db47aff49e4 287641805:5c23e8c447180399 287641805:651d3fc3874d1376\n\
             r 2147483647 1 -11 29 11:bdcf10e0a15685e0 0:b9d3c92fd6b36e91 -86662325:4b472be566ae9ff0 2:ba886c8904bafcaf 1:ba317151d2012caa 85:d8c9c4f3c5d26c7d 0:b9d3c92fd6b36e91 2:ba886c8904bafcaf 5:bbba1bd3fccc0f41 2060821323:b405afc5e6ae9ff0 2104152485:eb1d5b38a2494641 1318407084:bd243b2f91ce2fc4 694043488:277d0e7f015b9915\n\
             r 2147483647 1 0 0 2:baa594923fc1b14d 0:b9d3c92fd6b36e91 1676042610:81ee1e6309e7644d 0:b9d3fa0e8bdb98ce 2:ba8b40506f8c50b9 5:bbf43bf20ac78b03 0:b9d3c92fd6b36e91 0:b9d3fa0e8bdb98ce 9:bd261571ab11f18f 1676042610:968d3c7309e7644d 838021305:b28005d37628aeff 838021305:a3bb7f480c60b4ef 838021305:cf8b2942ad17887\n\
             r 2147483647 1 -2147483648 2147483647 15:bf480b1e857e6865 0:b9d3c92fd6b36e91 1133823247:e4829a79ce76e905 0:b9d6b36f67bdc4d3 5:bb973a69c4e61b94 23:c206a999b44e575b 0:b9d3c92fd6b36e91 0:b9d6b36f67bdc4d3 7:bc780717816e341b 1133823247:f91a728ace76e905 566911623:6395db0c8a6e5b5b 566911623:53d78c5ef9044a0a 566911623:15c8f6700e99f982\n\
             r 2147483647 2 -11 29 6:bbfb01691144c950 0:b9d3c92fd6b36e91 -2126773898:97fbebd9c667cd50 1:ba33e1020cd7fef0 0:b9d6cad5ad2b3089 99:dd14dafac0b22d68 0:b9d3c92fd6b36e91 1:ba33e1020cd7fef0 11:bdb80bc090b1d300 20709750:4ae378db4667cd50 1084096699:dbcde6c6eea90210 1084096699:cda54f15b847d402 1056800340:6ac4ab6150c0bd2e\n\
             r 2147483647 2 0 0 2:baa1074560c58d5f 0:b9d3c92fd6b36e91 1998755266:a9f2b16d0adbbe5f 2:ba8883db36282d4b 2:ba945db25fb17e5a 33:c637910964f218e4 0:b9d3c92fd6b36e91 2:ba8883db36282d4b 1:ba51adff77986608 1998755266:8716f6be0adbbe5f 999377633:8f0776995c287868 999377633:27856667c3ad68bc 999377633:1f38c15fbd5a3525\n\
             r 2147483647 2 -2147483648 2147483647 3:bb007b9198133204 0:b9d3c92fd6b36e91 1660584083:c9492bd61846e2c4 2:ba8e26a873f50311 5:bba0112dd2ee8758 41:c9462bc02b9a88ff 0:b9d3c92fd6b36e91 2:ba8e26a873f50311 9:bd0fedba00ddf4cb 1660584083:5c442df59846e2c4 830292041:545c5a32584f861b 830292041:b0360a4b5a044a2a 830292041:a8c11b24a463bafe\n"
        );

        let mut out = String::new();
        for size in [0, 1, 2, 3, 5, 8, 9, 10, 11, 12, 16, 23, 40] {
            out += &format!("b {size}");
            for radius in -1..=6 {
                let brushes = ErosionBrushes::new(size, radius);
                let mut fold = 17_i64;
                let mut points = 0;
                for cell in 0..size * size {
                    let (indices, weights) = brushes.get(cell);
                    fold = fold.wrapping_mul(31).wrapping_add(indices.len() as i64);
                    for (&index, weight) in indices.iter().zip(weights) {
                        fold = fold.wrapping_mul(31).wrapping_add(index as i64);
                        fold = fold
                            .wrapping_mul(31)
                            .wrapping_add(i64::from(weight.to_bits() as i32));
                        points += 1;
                    }
                }
                out += &format!(" {points}:{fold:x}");
            }
            out += "\n";
        }
        assert_eq!(
            out,
            "b 0 0:11 0:11 0:11 0:11 0:11 0:11 0:11 0:11\n\
             b 1 0:20f 0:20f 1:3f87be10 1:3f87be10 1:3f87be10 1:3f87be10 1:3f87be10 1:3f87be10\n\
             b 2 0:ef8f91 0:ef8f91 4:be657efc7c5bbb8f 16:1d4e852cc7b569d1 16:6e4df5c32590a91 16:d1a909535a8a1c91 16:438f4fdd7de5a411 16:6127d4aaed016791\n\
             b 3 0:198cb38e0830f 0:198cb38e0830f 9:f32bf2d7e670868c 49:3f878b51ab3b6ab2 81:c345e0104e801411 81:bf513ee9751da67c 81:27fdfdb3eb8970c6 81:c735797b83feb8d9\n\
             b 5 0:5d34b4191f6e850f 0:5d34b4191f6e850f 25:52c2a87fae135784 169:864b8c71c61d172a 361:26f73acab0cff2ac 513:ca7f81b8a1b3e654 605:930e8af82fc6892a 625:e76fad41ebf7d617\n\
             b 8 0:35e1b45929b6f811 0:35e1b45929b6f811 64:1cf122d8731f37f1 484:fd1e49586a71f34f 1156:6f4a0009cbc0ca9f 1836:4d51a5fc189b528d 2480:9db9530f93cc3d3f 3232:b84eaa2ada3fbe19\n\
             b 9 0:f9f0779819c80c0f 0:f9f0779819c80c0f 81:c559a38200a252e8 625:5b9182eb54218b8e 1521:71408d6f11f93630 2457:bd490478ab8a03c8 3381:5adc8ef442da1cd0 4537:65e433e6378107c5\n\
             b 10 0:ddadd1d5e9c00391 0:ddadd1d5e9c00391 100:6789b4f7390e0f5f 784:91a1e591662b9261 1936:bd3f274f9b9b1481 3168:1ca1ef0155af4d97 4420:317cfbaa82a93147 6060:aa095f3ddaf9cfb0\n\
             b 11 0:71359020712e910f 0:71359020712e910f 121:67a593acd1c73d54 961:5e0992aa10ba777a 2401:d4e51d27bca77afc 3969:318a1e71803a9f4 5597:8cec6594173e8f8c 7801:4959c29c64e46bb8\n\
             b 12 0:e27e71dcc115ae11 0:e27e71dcc115ae11 144:4658a553cf70bdc9 1156:4f1e9cf8516ef167 2916:c3f90c227b40c027 4860:f42dc452ab8cc7cd 6912:9ea2278b6ec4e0bb 9760:475002df5a775b3c\n\
             b 16 0:4562e551705be011 0:4562e551705be011 256:ab9dd70527dcdf91 2116:4560a0d282a8a74f 5476:2416c1e0adb74bf 9324:256878be4a19a9bd 13552:a8fc05f3c9a18de7 19776:cf69148ccf51470\n\
             b 23 0:c433b318f770440f 0:c433b318f770440f 529:6c7fcafb7cb92e08 4489:1b665d62c039052e 11881:16cb47cd48fcf050 20601:a665d4f3d1996108 30485:3fe67d4f6e3c2290 45697:27be5fc2d5a75f8c\n\
             b 40 0:53eda68c6dde3811 0:53eda68c6dde3811 1600:e2422ed16cc74f1 13924:9da19106999a7ecf 37636:cad9cdae5551dc9f 66348:3c313e4b659bdfcd 99760:2e5bb7ca2a1aa85f 153120:49681053cb99cab8\n"
        );
    }

    #[test]
    fn hydraulic_erosion_matches_engine_0_3_0() {
        struct ErosionMap {
            cells: Vec<Cell>,
        }

        impl Filterable for ErosionMap {
            fn block_x(&self) -> i32 {
                -32
            }

            fn block_z(&self) -> i32 {
                48
            }

            fn total_size(&self) -> usize {
                48
            }

            fn border_size(&self) -> usize {
                16
            }

            fn backing(&self) -> &[Cell] {
                &self.cells
            }

            fn backing_mut(&mut self) -> &mut [Cell] {
                &mut self.cells
            }
        }

        let registry = &TerrainRegistry::new();
        let total = 48;
        let cells = (0..total)
            .flat_map(|z| {
                (0..total).map(move |x| {
                    let index = z * total + x;
                    let mut cell = Cell::new(registry);
                    cell.value = 0.28 + ((x * 13 + z * 7 + x * z) % 31) as f32 / 1_000.0;
                    cell.river_mask = if index % 37 == 0 { 0.05 } else { 1.0 };
                    cell.terrain_region_edge = (index % 5) as f32 * 0.03;
                    cell.erosion_mask = index % 113 == 0;
                    cell
                })
            })
            .collect();
        let mut map = ErosionMap { cells };
        let levels = Levels::new(256, 63);
        Erosion::factory(12_345, total, ErosionSettings::default(), levels)
            .apply(&mut map, 99, -77, 2);

        let expected = [
            (12, 0x3e8fdec3, 0xb66fcb70, 0),
            (13, 0x3e968542, 0xb7745ea9, 0),
            (14, 0x3e9d12b7, 0xb96321be, 0),
            (15, 0x3e93c4b5, 0xb9cc66c8, 0),
            (16, 0x3e9a199e, 0xba862106, 0),
            (17, 0x3e90e4cf, 0xba83a375, 0),
            (18, 0x3e971f22, 0xbaf1407e, 0),
            (19, 0x3e9d7402, 0xbb22285d, 0),
            (20, 0x3e946e35, 0xbb0dde5f, 0x388ebcc2),
            (21, 0x3e9aa9d5, 0xbb3f9b74, 0x352c9cfa),
            (22, 0x3e9276a3, 0xbb01abbe, 0x3a8850a8),
            (23, 0x3e9851d4, 0xbb2420c7, 0x36754feb),
            (24, 0x3e8fec69, 0xbab2ec9e, 0x39f42625),
            (25, 0x3e965ad2, 0xbaaf8cde, 0x358992f7),
            (26, 0x3e9d21a4, 0xba9089d6, 0),
            (27, 0x3e9447d0, 0xb9cc4573, 0),
        ];
        for (index, value, erosion, sediment) in expected {
            assert_eq!(
                (
                    map.cells[index].value.to_bits(),
                    map.cells[index].erosion.to_bits(),
                    map.cells[index].sediment.to_bits(),
                ),
                (value, erosion, sediment),
                "cell {index}"
            );
        }
    }

    #[test]
    fn world_erosion_reuses_until_the_size_changes() {
        let builds = AtomicUsize::new(0);
        let cache = WorldErosion::new(
            |size| {
                builds.fetch_add(1, Ordering::Relaxed);
                size
            },
            |value, size| *value == size,
        );
        let first = cache.get(4_096);
        let reused = cache.get(4_096);
        assert!(Arc::ptr_eq(&first, &reused));
        assert_eq!(builds.load(Ordering::Relaxed), 1);

        let resized = cache.get(8_192);
        assert_eq!(*resized, 8_192);
        assert!(!Arc::ptr_eq(&first, &resized));
        assert_eq!(builds.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn world_erosion_builds_once_across_threads() {
        let builds = AtomicUsize::new(0);
        let cache = WorldErosion::new(
            |size| {
                builds.fetch_add(1, Ordering::Relaxed);
                std::thread::sleep(std::time::Duration::from_millis(5));
                size
            },
            |value, size| *value == size,
        );
        let values: Vec<Arc<i32>> = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..8).map(|_| scope.spawn(|| cache.get(4_096))).collect();
            handles.into_iter().map(|h| h.join().unwrap()).collect()
        });
        assert_eq!(builds.load(Ordering::Relaxed), 1);
        assert!(values.iter().all(|value| Arc::ptr_eq(value, &values[0])));
    }

    /// Replays `WorldErosionOracle.java` (`.local/terraforged/oracles/world_erosion/`), which calls Engine's
    /// `WorldErosion.get` with a validator that accepts any cached size at least as large as the context:
    /// each line is the context, the returned size, its build number and the factory and validator calls.
    #[test]
    fn world_erosion_calls_match_engine_oracle() {
        let calls = std::sync::Mutex::new(Vec::new());
        let builds = AtomicUsize::new(0);
        let cache = WorldErosion::new(
            |size: i32| {
                calls.lock().unwrap().push(format!("build {size}"));
                (size, builds.fetch_add(1, Ordering::Relaxed))
            },
            |value: &(i32, usize), context| {
                calls
                    .lock()
                    .unwrap()
                    .push(format!("validate {} {context}", value.0));
                value.0 >= context
            },
        );
        let mut out = String::new();
        for context in [
            4096,
            4096,
            1024,
            8192,
            8192,
            4096,
            -1,
            i32::MIN,
            i32::MAX,
            0,
        ] {
            let value = cache.get(context);
            let calls = std::mem::take(&mut *calls.lock().unwrap());
            out += &format!(
                "{context} -> {} #{} [{}]\n",
                value.0,
                value.1,
                calls.join(", ")
            );
        }
        assert_eq!(
            out,
            "4096 -> 4096 #0 [build 4096]\n\
             4096 -> 4096 #0 [validate 4096 4096]\n\
             1024 -> 4096 #0 [validate 4096 1024]\n\
             8192 -> 8192 #1 [validate 4096 8192, validate 4096 8192, build 8192]\n\
             8192 -> 8192 #1 [validate 8192 8192]\n\
             4096 -> 8192 #1 [validate 8192 4096]\n\
             -1 -> 8192 #1 [validate 8192 -1]\n\
             -2147483648 -> 8192 #1 [validate 8192 -2147483648]\n\
             2147483647 -> 2147483647 #2 [validate 8192 2147483647, validate 8192 2147483647, build 2147483647]\n\
             0 -> 2147483647 #2 [validate 2147483647 0]\n"
        );
    }

    fn pack_seed(x: i32, z: i32) -> i64 {
        (u64::from(x as u32) | (u64::from(z as u32) << 32)) as i64
    }
}
