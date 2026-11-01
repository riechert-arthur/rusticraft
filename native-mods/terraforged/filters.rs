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

pub trait Filterable {
    fn block_x(&self) -> i32;

    fn block_z(&self) -> i32;

    fn total_size(&self) -> usize;

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
struct RangeModifier {
    min: f32,
    max: f32,
    range: f32,
    inverted: bool,
}

impl RangeModifier {
    fn range(min: f32, max: f32) -> Self {
        Self {
            min,
            max,
            range: max - min,
            inverted: false,
        }
    }

    fn inverted(min: f32, max: f32) -> Self {
        Self {
            inverted: true,
            ..Self::range(min, max)
        }
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

    fn modify(self, cell: &Cell, value: f32) -> f32 {
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

#[derive(Clone, Copy, Debug)]
pub struct Smoothing {
    radius: usize,
    radius_squared: f32,
    strength: f32,
    modifier: RangeModifier,
}

impl Smoothing {
    pub fn new(settings: SmoothingSettings, levels: Levels) -> Self {
        Self {
            radius: java_round(settings.smoothing_radius + 0.5).max(0) as usize,
            radius_squared: settings.smoothing_radius * settings.smoothing_radius,
            strength: settings.smoothing_rate,
            modifier: RangeModifier::inverted(levels.ground(1), levels.ground(120)),
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
