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

use super::terraforged_engine::{
    Cell, ControlPoints, Levels, Terrain, TerrainProperties, TerrainRegistry,
};
use super::terraforged_tile::Tile;

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

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FilterSettings {
    pub erosion: ErosionSettings,
    pub smoothing: SmoothingSettings,
}

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

#[derive(Clone, Copy, Debug)]
pub struct Modifier {
    min: f32,
    max: f32,
    range: f32,
    inverted: bool,
}

impl Modifier {
    pub fn range(min: f32, max: f32) -> Self {
        Self {
            min,
            max,
            range: max - min,
            inverted: false,
        }
    }

    pub fn invert(mut self) -> Self {
        self.inverted = true;
        self
    }

    fn value(self, value: f32) -> f32 {
        let modifier = if value > self.max {
            1.0
        } else if value < self.min {
            0.0
        } else {
            (value - self.min) / self.range
        };
        if self.inverted {
            1.0 - modifier
        } else {
            modifier
        }
    }

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

#[derive(Clone, Copy, Debug)]
pub struct FastRandom {
    seed: u64,
    gamma: u64,
}

impl FastRandom {
    pub fn seeded(seed: i64, gamma: i64) -> Self {
        Self {
            seed: seed as u64,
            gamma: mix_gamma(gamma as u64),
        }
    }

    pub fn next_int(&mut self, bound: i32) -> i32 {
        let mut result = mix32(self.next_seed());
        let mask = bound - 1;
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

#[derive(Debug)]
struct ErosionBrush {
    indices: Vec<usize>,
    weights: Vec<f32>,
}

#[derive(Debug)]
pub struct ErosionBrushes {
    entries: Vec<ErosionBrush>,
}

impl ErosionBrushes {
    pub fn new(size: usize, radius: isize) -> Self {
        let capacity = (radius * radius * 4) as usize;
        let mut x_offsets = vec![0_isize; capacity];
        let mut y_offsets = vec![0_isize; capacity];
        let mut weights = vec![0.0_f32; capacity];
        let mut weight_sum = 0.0_f32;
        let mut add_index = 0;
        let mut entries = Vec::with_capacity(size * size);
        for index in 0..size * size {
            let center_x = index % size;
            let center_y = index / size;
            if center_y <= radius as usize
                || center_y >= size - radius as usize
                || center_x <= radius as usize + 1
                || center_x >= size - radius as usize
            {
                weight_sum = 0.0;
                add_index = 0;
                for y in -radius..=radius {
                    for x in -radius..=radius {
                        let squared_distance = (x * x + y * y) as f32;
                        if squared_distance >= (radius * radius) as f32 {
                            continue;
                        }
                        let coordinate_x = center_x as isize + x;
                        let coordinate_y = center_y as isize + y;
                        if coordinate_x < 0
                            || coordinate_x >= size as isize
                            || coordinate_y < 0
                            || coordinate_y >= size as isize
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
            let mut indices = Vec::with_capacity(add_index);
            let mut normalized_weights = Vec::with_capacity(add_index);
            for point in 0..add_index {
                indices.push(
                    ((y_offsets[point] + center_y as isize) * size as isize
                        + x_offsets[point]
                        + center_x as isize) as usize,
                );
                normalized_weights.push(weights[point] / weight_sum);
            }
            entries.push(ErosionBrush {
                indices,
                weights: normalized_weights,
            });
        }
        Self { entries }
    }

    pub fn get(&self, index: usize) -> (&[usize], &[f32]) {
        let brush = &self.entries[index];
        (&brush.indices, &brush.weights)
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

#[derive(Clone, Copy, Debug)]
pub struct Smoothing {
    radius: usize,
    radius_squared: f32,
    strength: f32,
    modifier: Modifier,
}

impl Smoothing {
    pub fn new(settings: SmoothingSettings, levels: Levels) -> Self {
        Self {
            radius: java_round(settings.smoothing_radius + 0.5).max(0) as usize,
            radius_squared: settings.smoothing_radius * settings.smoothing_radius,
            strength: settings.smoothing_rate,
            modifier: Modifier::range(levels.ground(1), levels.ground(120)).invert(),
        }
    }

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
        let Some(max) = map.total_size().checked_sub(self.radius) else {
            return;
        };
        for z in self.radius..max {
            for x in self.radius..max {
                let cell = map
                    .cell_raw(x, z)
                    .expect("filter coordinates are in bounds");
                if cell.erosion_mask {
                    continue;
                }
                let mut total = 0.0_f32;
                let mut weights = 0.0_f32;
                for dz in -(self.radius as isize)..=self.radius as isize {
                    for dx in -(self.radius as isize)..=self.radius as isize {
                        let distance_squared = (dx * dx + dz * dz) as f32;
                        if distance_squared > self.radius_squared {
                            continue;
                        }
                        let neighbour = map
                            .cell_raw(x.wrapping_add_signed(dx), z.wrapping_add_signed(dz))
                            .expect("smoothing radius stays inside the filter map");
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

#[derive(Clone, Copy, Debug)]
pub struct Steepness {
    radius: isize,
    scaler: f32,
    water_level: f32,
    _max_beach_level: f32,
}

impl Steepness {
    pub fn new(radius: i32, scaler: f32, levels: Levels) -> Self {
        Self {
            radius: radius as isize,
            scaler,
            water_level: levels.water,
            _max_beach_level: levels.water(6),
        }
    }

    pub fn apply(&self, map: &mut impl Filterable, _seed_x: i32, _seed_z: i32, _iterations: i32) {
        let total = map.total_size();
        for z in 0..total {
            for x in 0..total {
                let cell_value = map
                    .cell_raw(x, z)
                    .expect("filter coordinates are in bounds")
                    .value;
                let mut total_height_difference = 0.0_f32;
                for dz in -1_isize..=2 {
                    for dx in -1_isize..=2 {
                        if dx == 0 && dz == 0 {
                            continue;
                        }
                        let Some(neighbour) = map.cell_raw(
                            x.wrapping_add_signed(dx * self.radius),
                            z.wrapping_add_signed(dz * self.radius),
                        ) else {
                            continue;
                        };
                        if neighbour.is_absent() {
                            continue;
                        }
                        let height = java_max(neighbour.value, self.water_level);
                        total_height_difference += (cell_value - height).abs() / self.radius as f32;
                    }
                }
                map.cell_raw_mut(x, z)
                    .expect("filter coordinates are in bounds")
                    .gradient = java_min(1.0, total_height_difference * self.scaler);
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct BeachDetect {
    transition: ControlPoints,
    _gradient_squared: f32,
    radius: usize,
    diameter: i32,
    beach: Terrain,
}

impl BeachDetect {
    pub fn new(transition: ControlPoints, terrains: &TerrainRegistry) -> Self {
        let gradient = 0.001_838_235_3_f32;
        Self {
            transition,
            _gradient_squared: gradient * gradient,
            radius: 8,
            diameter: 17,
            beach: terrains.get_by_id(4),
        }
    }

    pub fn apply(&self, map: &mut impl Filterable, _seed_x: i32, _seed_z: i32, _iterations: i32) {
        let total = map.total_size();
        for z in 0..total {
            for x in 0..total {
                let cell = map
                    .cell_raw(x, z)
                    .expect("filter coordinates are in bounds");
                if !cell.terrain.is_coast() || cell.continent_edge >= self.transition.beach {
                    continue;
                }
                let north = map.cell_raw(x, z.wrapping_sub(self.radius));
                let south = map.cell_raw(x, z.wrapping_add(self.radius));
                let east = map.cell_raw(x.wrapping_add(self.radius), z);
                let west = map.cell_raw(x.wrapping_sub(self.radius), z);
                let gradient_x = self.gradient(east, west, cell);
                let gradient_z = self.gradient(north, south, cell);
                let distance_squared = gradient_x * gradient_x + gradient_z * gradient_z;
                if distance_squared < 0.275 {
                    map.cell_raw_mut(x, z)
                        .expect("filter coordinates are in bounds")
                        .terrain = self.beach.clone();
                }
            }
        }
    }

    fn gradient(&self, first: Option<&Cell>, second: Option<&Cell>, default: &Cell) -> f32 {
        let mut distance = self.diameter;
        let first = match first.filter(|cell| !cell.is_absent()) {
            Some(cell) => cell,
            None => {
                distance -= self.radius as i32;
                default
            }
        };
        let second = match second.filter(|cell| !cell.is_absent()) {
            Some(cell) => cell,
            None => {
                distance -= self.radius as i32;
                default
            }
        };
        (first.value - second.value) / distance as f32
    }
}

fn java_round(value: f32) -> i32 {
    (value + 0.5).floor() as i32
}

fn map(value: f32, min: f32, max: f32, range: f32) -> f32 {
    let difference = value.clamp(min, max) - min;
    if difference > range {
        1.0
    } else {
        difference / range
    }
}

fn lerp(first: f32, second: f32, alpha: f32) -> f32 {
    first + alpha * (second - first)
}

fn java_min(first: f32, second: f32) -> f32 {
    if first.is_nan() || second.is_nan() {
        f32::NAN
    } else if first <= second {
        first
    } else {
        second
    }
}

fn java_max(first: f32, second: f32) -> f32 {
    if first.is_nan() || second.is_nan() {
        f32::NAN
    } else if first >= second {
        first
    } else {
        second
    }
}

pub struct WorldErosion<T, F, V> {
    value: RwLock<Option<Arc<T>>>,
    factory: F,
    validator: V,
}

type ErosionFactory = Box<dyn Fn(i32) -> Erosion + Send + Sync>;
type ErosionValidator = fn(&Erosion, i32) -> bool;

pub struct WorldFilters {
    smoothing: Smoothing,
    steepness: Steepness,
    beach: BeachDetect,
    settings: FilterSettings,
    erosion: WorldErosion<Erosion, ErosionFactory, ErosionValidator>,
    erosion_iterations: i32,
    smoothing_iterations: i32,
}

impl WorldFilters {
    pub fn new(
        root_seed: i32,
        settings: FilterSettings,
        levels: Levels,
        transition: ControlPoints,
        terrains: &TerrainRegistry,
    ) -> Self {
        let erosion_settings = settings.erosion;
        let factory: ErosionFactory = Box::new(move |size| {
            Erosion::factory(root_seed, size as usize, erosion_settings, levels)
        });
        let validator: ErosionValidator = |erosion, size| erosion.size() == size as usize;
        Self {
            smoothing: Smoothing::new(settings.smoothing, levels),
            steepness: Steepness::new(1, 10.0, levels),
            beach: BeachDetect::new(transition, terrains),
            settings,
            erosion: WorldErosion::new(factory, validator),
            erosion_iterations: settings.erosion.droplets_per_chunk,
            smoothing_iterations: settings.smoothing.iterations,
        }
    }

    pub fn settings(&self) -> &FilterSettings {
        &self.settings
    }

    pub fn apply(&self, map: &mut impl Filterable, region_x: i32, region_z: i32, optional: bool) {
        if optional {
            self.apply_optional(map, region_x, region_z);
        }
        self.apply_required(map, region_x, region_z);
    }

    pub fn apply_tile(&self, tile: &mut Tile, optional: bool) {
        let region_x = tile.region_x();
        let region_z = tile.region_z();
        self.apply(&mut tile.filterable(), region_x, region_z, optional);
    }

    pub fn apply_required(&self, map: &mut impl Filterable, seed_x: i32, seed_z: i32) {
        self.steepness.apply(map, seed_x, seed_z, 1);
        self.beach.apply(map, seed_x, seed_z, 1);
    }

    pub fn apply_optional(&self, map: &mut impl Filterable, seed_x: i32, seed_z: i32) {
        self.erosion.get(map.total_size() as i32).apply(
            map,
            seed_x,
            seed_z,
            self.erosion_iterations,
        );
        self.smoothing
            .apply(map, seed_x, seed_z, self.smoothing_iterations);
    }
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

    pub fn get(&self, context: i32) -> Arc<T> {
        if let Some(value) = self.read_valid(context) {
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

    fn read_valid(&self, context: i32) -> Option<Arc<T>> {
        self.value
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .as_ref()
            .filter(|value| (self.validator)(value, context))
            .map(Arc::clone)
    }
}

#[cfg(test)]
mod tests {
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

    #[test]
    fn smoothing_matches_engine_0_3_0() {
        let total = 7;
        let mut cells = (0..total * total)
            .map(|index| {
                let mut cell = Cell::default();
                cell.value = ((index * 37 + 11) % 101) as f32 / 100.0;
                cell.river_mask = if index % 9 == 0 { 0.05 } else { 1.0 };
                cell.terrain_region_edge = (index % 5) as f32 * 0.04;
                cell.erosion_mask = index == 24;
                cell
            })
            .collect::<Vec<_>>();
        cells[17] = Cell::empty();
        let mut map = TestMap { total, cells };

        Smoothing::new(SmoothingSettings::default(), Levels::new(256, 63))
            .apply(&mut map, 123, -456, 2);

        let expected = [
            (16, 0x3f7ae148),
            (18, 0x3f35299f),
            (23, 0x3f105097),
            (24, 0x3f68f5c3),
            (25, 0x3f0ba1bc),
            (30, 0x3eec95ee),
            (32, 0x3f570a3d),
        ];
        for (index, bits) in expected {
            assert_eq!(map.cells[index].value.to_bits(), bits, "cell {index}");
        }
    }

    #[test]
    fn steepness_matches_engine_0_3_0() {
        let total = 6;
        let cells = (0..total * total)
            .map(|index| {
                let mut cell = Cell::default();
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

    #[test]
    fn beach_detection_matches_engine_0_3_0() {
        let total = 20;
        let mut cells = (0..total * total)
            .map(|index| {
                let mut cell = Cell::default();
                cell.value = 0.25 + ((index * 17 + 3) % 41) as f32 / 512.0;
                cell
            })
            .collect::<Vec<_>>();
        let terrains = TerrainRegistry::new();
        let smooth = 10 + 10 * total;
        let steep = 9 + 9 * total;
        let inland = 11 + 10 * total;
        let other = 12 + 10 * total;
        for index in [smooth, steep, inland] {
            cells[index].terrain = terrains.get_by_id(3);
            cells[index].continent_edge = 0.2;
        }
        cells[inland].continent_edge = 0.4;
        cells[17 + 9 * total].value = 10.0;
        cells[1 + 9 * total].value = -10.0;
        let mut map = TestMap { total, cells };

        BeachDetect::new(ControlPoints::default(), &terrains).apply(&mut map, 123, -456, 99);

        assert_eq!(
            [smooth, steep, inland, other].map(|index| map.cells[index].terrain.id()),
            [4, 3, 3, 0]
        );
    }

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
            let brush = &brushes.entries[cell];
            let points = [0, brush.indices.len() / 2, brush.indices.len() - 1];
            assert_eq!(
                points.map(|point| (brush.indices[point], brush.weights[point].to_bits())),
                expected,
                "brush {cell}"
            );
        }
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

        let total = 48;
        let cells = (0..total)
            .flat_map(|z| {
                (0..total).map(move |x| {
                    let index = z * total + x;
                    let mut cell = Cell::default();
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
    fn world_filters_match_engine_0_3_0() {
        struct FilterMap {
            cells: Vec<Cell>,
        }

        impl Filterable for FilterMap {
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

        let total = 48;
        let terrains = TerrainRegistry::new();
        let cells = (0..total)
            .flat_map(|z| {
                let coast = terrains.get_by_id(3);
                (0..total).map(move |x| {
                    let index = z * total + x;
                    let mut cell = Cell::default();
                    cell.value = 0.28 + ((x * 13 + z * 7 + x * z) % 31) as f32 / 1_000.0;
                    cell.continent_edge = 0.2;
                    if index % 11 == 0 {
                        cell.terrain = coast.clone();
                    }
                    cell.erosion_mask = index % 113 == 0;
                    cell
                })
            })
            .collect();
        let mut map = FilterMap { cells };
        let settings = FilterSettings {
            erosion: ErosionSettings {
                droplets_per_chunk: 2,
                ..ErosionSettings::default()
            },
            smoothing: SmoothingSettings {
                iterations: 1,
                ..SmoothingSettings::default()
            },
        };
        let filters = WorldFilters::new(
            0,
            settings,
            Levels::new(256, 63),
            ControlPoints::default(),
            &terrains,
        );

        filters.apply_optional(&mut map, 2, -3);
        filters.apply_required(&mut map, 2, -3);

        let expected = [
            (0, 0x3e8f5c29, 0, 0, 0x3f800000, 4),
            (11, 0x3e991687, 0, 0, 0x3f50b209, 4),
            (12, 0x3e8fdf3b, 0, 0, 0x3f800000, 0),
            (113, 0x3e9a1cac, 0, 0, 0x3f4d2ce7, 0),
            (210, 0x3e981df5, 0, 0, 0x3f05f840, 0),
            (511, 0x3e94b44c, 0xb8f1d340, 0, 0x3f255b54, 0),
            (1024, 0x3e950493, 0xb8bdffce, 0, 0x3f149f6c, 0),
            (1536, 0x3e92d459, 0xb96a8c37, 0, 0x3f800000, 0),
            (2299, 0x3e9cac08, 0, 0, 0x3f667981, 4),
        ];
        for (index, value, erosion, sediment, gradient, terrain) in expected {
            let cell = &map.cells[index];
            assert_eq!(
                (
                    cell.value.to_bits(),
                    cell.erosion.to_bits(),
                    cell.sediment.to_bits(),
                    cell.gradient.to_bits(),
                    cell.terrain.id(),
                ),
                (value, erosion, sediment, gradient, terrain),
                "cell {index}"
            );
        }
    }

    #[test]
    fn world_filters_apply_to_tiles() {
        let terrains = TerrainRegistry::new();
        let settings = FilterSettings {
            erosion: ErosionSettings {
                droplets_per_chunk: 2,
                ..ErosionSettings::default()
            },
            smoothing: SmoothingSettings::default(),
        };
        let filters = WorldFilters::new(
            0,
            settings,
            Levels::new(256, 63),
            ControlPoints::default(),
            &terrains,
        );
        let mut tile = Tile::new(-2, 3, 0, 1);
        {
            let mut map = tile.filterable();
            for z in 0..map.total_size() {
                for x in 0..map.total_size() {
                    let index = z * map.total_size() + x;
                    let cell = map.cell_raw_mut(x, z).unwrap();
                    cell.value = 0.28 + ((x * 13 + z * 7 + x * z) % 31) as f32 / 1_000.0;
                    cell.continent_edge = 0.2;
                    if index % 11 == 0 {
                        cell.terrain = terrains.get_by_id(3);
                    }
                    cell.erosion_mask = index % 113 == 0;
                }
            }
        }

        filters.apply_tile(&mut tile, true);

        let map = tile.filterable();
        for (index, value, erosion, gradient, terrain) in [
            (0, 0x3e8f5c29, 0, 0x3f800000, 4),
            (11, 0x3e991687, 0, 0x3f50b209, 4),
            (511, 0x3e94b44c, 0xb8f1d340, 0x3f255b54, 0),
            (1024, 0x3e950493, 0xb8bdffce, 0x3f149f6c, 0),
            (2299, 0x3e9cac08, 0, 0x3f667981, 4),
        ] {
            let cell = &map.backing()[index];
            assert_eq!(
                (
                    cell.value.to_bits(),
                    cell.erosion.to_bits(),
                    cell.gradient.to_bits(),
                    cell.terrain.id(),
                ),
                (value, erosion, gradient, terrain),
                "cell {index}"
            );
        }
    }

    #[test]
    fn world_erosion_matches_engine_0_3_0() {
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

    fn pack_seed(x: i32, z: i32) -> i64 {
        (u64::from(x as u32) | (u64::from(z as u32) << 32)) as i64
    }
}
