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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NoiseTileSize {
    pub chunk_length: i32,
    pub chunk_size: i32,
    pub chunk_min: i32,
    pub chunk_max: i32,
    pub region_length: i32,
    pub region_size: i32,
    pub min: i32,
    pub max: i32,
}

impl NoiseTileSize {
    pub const CHUNK_SIZE: i32 = 16;
    pub const DEFAULT: Self = Self::new(1);

    pub const fn new(chunk_radius: i32) -> Self {
        let chunk_length = 1_i32.wrapping_add(chunk_radius.wrapping_mul(2));
        let chunk_size = chunk_length.wrapping_mul(chunk_length);
        let chunk_min = chunk_radius.wrapping_neg();
        let chunk_max = chunk_min.wrapping_add(chunk_length);
        let region_length = chunk_length.wrapping_mul(Self::CHUNK_SIZE);
        let region_size = region_length.wrapping_mul(region_length);
        let min = Self::CHUNK_SIZE.wrapping_mul(chunk_radius).wrapping_neg();
        let max = min.wrapping_add(region_length);
        Self {
            chunk_length,
            chunk_size,
            chunk_min,
            chunk_max,
            region_length,
            region_size,
            min,
            max,
        }
    }

    pub const fn chunk_index_of(self, x: i32, z: i32) -> i32 {
        z.wrapping_mul(self.chunk_length).wrapping_add(x)
    }

    pub const fn chunk_index_of_rel(self, x: i32, z: i32) -> i32 {
        self.chunk_index_of(
            x.wrapping_sub(self.chunk_min),
            z.wrapping_sub(self.chunk_min),
        )
    }

    pub const fn index_of(self, x: i32, z: i32) -> i32 {
        z.wrapping_mul(self.region_length).wrapping_add(x)
    }

    pub const fn index_of_rel(self, x: i32, z: i32) -> i32 {
        self.index_of(x.wrapping_sub(self.min), z.wrapping_sub(self.min))
    }
}

impl std::fmt::Display for NoiseTileSize {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "NoiseTileSize{{chunkDiameter={}, chunkSize={}, chunkMin={}, chunkMax={}, regionDiameter={}, regionSize={}, min={}, max={}}}",
            self.chunk_length,
            self.chunk_size,
            self.chunk_min,
            self.chunk_max,
            self.region_length,
            self.region_size,
            self.min,
            self.max
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HeightmapSmoothingSettings {
    pub iterations: i32,
    pub smoothing_radius: f32,
    pub smoothing_rate: f32,
}

impl Default for HeightmapSmoothingSettings {
    fn default() -> Self {
        Self {
            iterations: 0,
            smoothing_radius: 1.75,
            smoothing_rate: 0.85,
        }
    }
}

pub struct HeightmapSmoothing;

impl HeightmapSmoothing {
    pub fn apply(map: &mut [f32], length: i32, settings: HeightmapSmoothingSettings) {
        let mut iterations = settings.iterations;
        if iterations <= 0 {
            return;
        }
        let radius_value = settings.smoothing_radius;
        let radius = java_round(radius_value + 0.5);
        let radius_squared = radius_value * radius_value;
        let strength = settings.smoothing_rate;
        if radius <= 0 || radius_squared <= 0.0 || strength <= 0.0 {
            return;
        }
        while iterations > 0 {
            for z in radius..length.wrapping_sub(radius) {
                for x in radius..length.wrapping_sub(radius) {
                    let mut total = 0.0_f32;
                    let mut weights = 0.0_f32;
                    for delta_z in -radius..=radius {
                        for delta_x in -radius..=radius {
                            let distance_squared = delta_x
                                .wrapping_mul(delta_x)
                                .wrapping_add(delta_z.wrapping_mul(delta_z))
                                as f32;
                            if distance_squared > radius_squared {
                                continue;
                            }
                            let weight = 1.0 - distance_squared / radius_squared;
                            let index = z
                                .wrapping_add(delta_z)
                                .wrapping_mul(length)
                                .wrapping_add(x.wrapping_add(delta_x));
                            total += map[index as usize] * weight;
                            weights += weight;
                        }
                    }
                    if weights > 0.0 {
                        let index = z.wrapping_mul(length).wrapping_add(x) as usize;
                        map[index] -= (map[index] - total / weights) * strength;
                    }
                }
            }
            iterations -= 1;
        }
    }
}

fn java_round(value: f32) -> i32 {
    if value >= 0.0 {
        (value + 0.5) as i32
    } else {
        (value - 0.5) as i32
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ErosionFilterSettings {
    pub droplets_per_chunk: i32,
    pub droplet_lifetime: i32,
    pub droplet_volume: f32,
    pub droplet_velocity: f32,
    pub erosion_rate: f32,
    pub deposit_rate: f32,
}

impl Default for ErosionFilterSettings {
    fn default() -> Self {
        Self {
            droplets_per_chunk: 350,
            droplet_lifetime: 25,
            droplet_volume: 0.7,
            droplet_velocity: 0.7,
            erosion_rate: 0.5,
            deposit_rate: 0.5,
        }
    }
}

struct ErosionDisc {
    x: Vec<i32>,
    y: Vec<i32>,
    weights: Vec<f32>,
    weight_sum: f32,
    reach: i32,
}

impl ErosionDisc {
    fn new(radius: i32) -> Self {
        let mut x = Vec::new();
        let mut y = Vec::new();
        let mut weights = Vec::new();
        let mut weight_sum = 0.0_f32;
        let mut reach = 0;
        for delta_y in -radius..=radius {
            for delta_x in -radius..=radius {
                let squared_distance = delta_x
                    .wrapping_mul(delta_x)
                    .wrapping_add(delta_y.wrapping_mul(delta_y))
                    as f32;
                if squared_distance < radius.wrapping_mul(radius) as f32 {
                    let weight = 1.0 - f64::from(squared_distance).sqrt() as f32 / radius as f32;
                    weight_sum += weight;
                    x.push(delta_x);
                    y.push(delta_y);
                    weights.push(weight);
                    reach = reach.max(delta_x.abs().max(delta_y.abs()));
                }
            }
        }
        Self {
            x,
            y,
            weights,
            weight_sum,
            reach,
        }
    }
}

pub struct ErosionBrushes {
    offsets: Vec<i32>,
    weights: Vec<f32>,
    edge_offsets: Vec<Option<Vec<i32>>>,
    edge_weights: Vec<Option<Vec<f32>>>,
}

#[derive(Default)]
pub struct ErosionResource {
    gradient1: [f32; 3],
    gradient2: [f32; 3],
}

pub struct ErosionFilter {
    erode_speed: f32,
    deposit_speed: f32,
    initial_speed: f32,
    initial_water_volume: f32,
    max_droplet_lifetime: i32,
    iterations: i32,
    brushes: ErosionBrushes,
}

impl ErosionFilter {
    const HEIGHT_FALL_OFF: f32 = 0.4;
    const INERTIA: f32 = 0.005;
    const SEDIMENT_CAPACITY_FACTOR: f32 = 7.0;
    const MIN_SEDIMENT_CAPACITY: f32 = 0.008;
    const EVAPORATE_SPEED: f32 = 0.35;
    const GRAVITY: f32 = 2.5;

    pub fn new(map_size: i32, settings: ErosionFilterSettings) -> Self {
        Self {
            erode_speed: settings.erosion_rate,
            deposit_speed: settings.deposit_rate,
            initial_speed: settings.droplet_velocity,
            initial_water_volume: settings.droplet_volume,
            max_droplet_lifetime: settings.droplet_lifetime,
            iterations: settings.droplets_per_chunk,
            brushes: ErosionBrushes::new(map_size),
        }
    }

    pub fn apply_region(
        &self,
        seed: i32,
        origin_chunk_x: i32,
        origin_chunk_z: i32,
        chunks_across: i32,
        map_size: i32,
        resource: &mut ErosionResource,
        map: &mut [f32],
    ) {
        let max_index = map_size.wrapping_sub(2);
        for iteration in 0..self.iterations {
            let iteration_seed = pack_seed(seed, iteration);
            for chunk_z in 0..chunks_across {
                let start_z = chunk_z.wrapping_shl(4);
                for chunk_x in 0..chunks_across {
                    let start_x = chunk_x.wrapping_shl(4);
                    let chunk_seed = pack_seed(
                        origin_chunk_x.wrapping_add(chunk_x),
                        origin_chunk_z.wrapping_add(chunk_z),
                    );
                    let mut random =
                        super::terraforged_filters::FastRandom::seeded(chunk_seed, iteration_seed);
                    let x = clamp_i32(
                        start_x.wrapping_add(random.next_int(NoiseTileSize::CHUNK_SIZE)),
                        1,
                        max_index,
                    );
                    let z = clamp_i32(
                        start_z.wrapping_add(random.next_int(NoiseTileSize::CHUNK_SIZE)),
                        1,
                        max_index,
                    );
                    self.apply_drop(x as f32, z as f32, map, map_size, resource);
                }
            }
        }
    }

    pub fn apply_tile(
        &self,
        seed: i32,
        chunk_x: i32,
        chunk_z: i32,
        size: NoiseTileSize,
        resource: &mut ErosionResource,
        map: &mut [f32],
    ) {
        let max_index = size.region_length.wrapping_sub(2);
        for iteration in 0..self.iterations {
            let iteration_seed = pack_seed(seed, iteration);
            for delta_z in size.chunk_min..size.chunk_max {
                let start_z = delta_z.wrapping_sub(size.chunk_min).wrapping_shl(4);
                for delta_x in size.chunk_min..size.chunk_max {
                    let start_x = delta_x.wrapping_sub(size.chunk_min).wrapping_shl(4);
                    let chunk_seed =
                        pack_seed(chunk_x.wrapping_add(delta_x), chunk_z.wrapping_add(delta_z));
                    let mut random =
                        super::terraforged_filters::FastRandom::seeded(chunk_seed, iteration_seed);
                    let x = clamp_i32(
                        start_x.wrapping_add(random.next_int(NoiseTileSize::CHUNK_SIZE)),
                        1,
                        max_index,
                    );
                    let z = clamp_i32(
                        start_z.wrapping_add(random.next_int(NoiseTileSize::CHUNK_SIZE)),
                        1,
                        max_index,
                    );
                    self.apply_drop(x as f32, z as f32, map, size.region_length, resource);
                }
            }
        }
    }

    fn apply_drop(
        &self,
        mut position_x: f32,
        mut position_y: f32,
        map: &mut [f32],
        map_size: i32,
        resource: &mut ErosionResource,
    ) {
        let mut direction_x = 0.0_f32;
        let mut direction_y = 0.0_f32;
        let mut sediment = 0.0_f32;
        let mut speed = self.initial_speed;
        let mut water = self.initial_water_volume;
        for _ in 0..self.max_droplet_lifetime {
            let node_x = position_x as i32;
            let node_y = position_y as i32;
            let droplet_index = node_y.wrapping_mul(map_size).wrapping_add(node_x);
            let cell_offset_x = position_x - node_x as f32;
            let cell_offset_y = position_y - node_y as f32;
            gradient(
                map,
                map_size,
                position_x,
                position_y,
                &mut resource.gradient1,
            );
            let first = resource.gradient1;
            direction_x = direction_x * Self::INERTIA - first[1] * (1.0 - Self::INERTIA);
            direction_y = direction_y * Self::INERTIA - first[2] * (1.0 - Self::INERTIA);
            let length_squared = direction_x * direction_x + direction_y * direction_y;
            if length_squared == 0.0 {
                return;
            }
            let length = f64::from(length_squared).sqrt() as f32;
            direction_x /= length;
            direction_y /= length;
            position_x += direction_x;
            position_y += direction_y;
            if (direction_x == 0.0 && direction_y == 0.0)
                || position_x < 0.0
                || position_x >= map_size.wrapping_sub(1) as f32
                || position_y < 0.0
                || position_y >= map_size.wrapping_sub(1) as f32
            {
                return;
            }
            let falloff = height_falloff(map[droplet_index as usize]);
            gradient(
                map,
                map_size,
                position_x,
                position_y,
                &mut resource.gradient2,
            );
            let delta_height = (resource.gradient2[0] - first[0]) * falloff;
            let sediment_capacity = java_max(
                -delta_height * speed * water * Self::SEDIMENT_CAPACITY_FACTOR,
                Self::MIN_SEDIMENT_CAPACITY,
            );
            if sediment > sediment_capacity || delta_height > 0.0 {
                let deposit = if delta_height > 0.0 {
                    java_min(delta_height, sediment)
                } else {
                    (sediment - sediment_capacity) * self.deposit_speed
                };
                sediment -= deposit;
                let index = droplet_index as usize;
                let width = map_size as usize;
                map[index] += deposit * (1.0 - cell_offset_x) * (1.0 - cell_offset_y);
                map[index + 1] += deposit * cell_offset_x * (1.0 - cell_offset_y);
                map[index + width] += deposit * (1.0 - cell_offset_x) * cell_offset_y;
                map[index + width + 1] += deposit * cell_offset_x * cell_offset_y;
            } else {
                let erosion = java_min(
                    (sediment_capacity - sediment) * self.erode_speed,
                    -delta_height,
                );
                let (offsets, weights) = self.brushes.brush(droplet_index as usize);
                for (&offset, &weight) in offsets.iter().zip(weights) {
                    let index = droplet_index.wrapping_add(offset) as usize;
                    let amount = erosion * weight;
                    let sediment_delta = java_min(map[index], amount);
                    map[index] -= sediment_delta;
                    sediment += sediment_delta;
                }
            }
            let speed_squared = speed * speed + delta_height * Self::GRAVITY;
            if speed_squared <= 0.0 {
                return;
            }
            speed = f64::from(speed_squared).sqrt() as f32;
            water *= 1.0 - Self::EVAPORATE_SPEED;
        }
    }
}

fn gradient(map: &[f32], map_size: i32, position_x: f32, position_y: f32, output: &mut [f32; 3]) {
    let x_coordinate = position_x as i32;
    let y_coordinate = position_y as i32;
    let x = position_x - x_coordinate as f32;
    let y = position_y - y_coordinate as f32;
    let northwest = y_coordinate
        .wrapping_mul(map_size)
        .wrapping_add(x_coordinate) as usize;
    let width = map_size as usize;
    let height_northwest = map[northwest];
    let height_northeast = map[northwest + 1];
    let height_southwest = map[northwest + width];
    let height_southeast = map[northwest + width + 1];
    output[0] = height_northwest * (1.0 - x) * (1.0 - y)
        + height_northeast * x * (1.0 - y)
        + height_southwest * (1.0 - x) * y
        + height_southeast * x * y;
    output[1] = (height_northeast - height_northwest) * (1.0 - y)
        + (height_southeast - height_southwest) * y;
    output[2] = (height_southwest - height_northwest) * (1.0 - x)
        + (height_southeast - height_northeast) * x;
}

fn height_falloff(height: f32) -> f32 {
    if height >= ErosionFilter::HEIGHT_FALL_OFF {
        1.0
    } else {
        height / ErosionFilter::HEIGHT_FALL_OFF
    }
}

fn pack_seed(x: i32, z: i32) -> i64 {
    (u64::from(x as u32) | (u64::from(z as u32) << 32)) as i64
}

fn clamp_i32(value: i32, minimum: i32, maximum: i32) -> i32 {
    if value < minimum {
        minimum
    } else if value > maximum {
        maximum
    } else {
        value
    }
}

fn java_max(first: f32, second: f32) -> f32 {
    if first.is_nan() {
        first
    } else if second.is_nan() {
        second
    } else if first == 0.0 && second == 0.0 {
        if first.is_sign_positive() || second.is_sign_positive() {
            0.0
        } else {
            -0.0
        }
    } else if first >= second {
        first
    } else {
        second
    }
}

fn java_min(first: f32, second: f32) -> f32 {
    if first.is_nan() {
        first
    } else if second.is_nan() {
        second
    } else if first == 0.0 && second == 0.0 {
        if first.is_sign_negative() || second.is_sign_negative() {
            -0.0
        } else {
            0.0
        }
    } else if first <= second {
        first
    } else {
        second
    }
}

impl ErosionBrushes {
    pub const RADIUS: i32 = 7;

    pub fn new(map_size: i32) -> Self {
        let disc = ErosionDisc::new(Self::RADIUS);
        let offsets = disc
            .x
            .iter()
            .zip(&disc.y)
            .map(|(&x, &y)| y.wrapping_mul(map_size).wrapping_add(x))
            .collect();
        let weights = disc
            .weights
            .iter()
            .map(|weight| weight / disc.weight_sum)
            .collect();
        let length = map_size.wrapping_mul(map_size) as usize;
        let mut brushes = Self {
            offsets,
            weights,
            edge_offsets: std::iter::repeat_with(|| None).take(length).collect(),
            edge_weights: std::iter::repeat_with(|| None).take(length).collect(),
        };
        brushes.init_edges(map_size, &disc);
        brushes
    }

    pub fn brush(&self, index: usize) -> (&[i32], &[f32]) {
        match (&self.edge_offsets[index], &self.edge_weights[index]) {
            (Some(offsets), Some(weights)) => (offsets, weights),
            _ => (&self.offsets, &self.weights),
        }
    }

    fn init_edges(&mut self, size: i32, disc: &ErosionDisc) {
        for index in 0..self.edge_offsets.len() {
            let center_x = index as i32 % size;
            let center_y = index as i32 / size;
            let clipped = center_x < disc.reach
                || center_x >= size - disc.reach
                || center_y < disc.reach
                || center_y >= size - disc.reach;
            if !clipped {
                continue;
            }
            let mut weight_sum = 0.0_f32;
            for point in 0..disc.weights.len() {
                let x = center_x.wrapping_add(disc.x[point]);
                let y = center_y.wrapping_add(disc.y[point]);
                if x >= 0 && x < size && y >= 0 && y < size {
                    weight_sum += disc.weights[point];
                }
            }
            let mut offsets = Vec::new();
            let mut weights = Vec::new();
            for point in 0..disc.weights.len() {
                let x = center_x.wrapping_add(disc.x[point]);
                let y = center_y.wrapping_add(disc.y[point]);
                if x >= 0 && x < size && y >= 0 && y < size {
                    offsets.push(disc.y[point].wrapping_mul(size).wrapping_add(disc.x[point]));
                    weights.push(disc.weights[point] / weight_sum);
                }
            }
            self.edge_offsets[index] = Some(offsets);
            self.edge_weights[index] = Some(weights);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noise_tile_sizes_match_current_java_source() {
        let fixtures = [
            (0, [1, 1, 0, 1, 16, 256, 0, 16], [-6, 0, -75, -75]),
            (1, [3, 9, -1, 2, 48, 2304, -16, 32], [-4, 6, -203, 581]),
            (2, [5, 25, -2, 3, 80, 6400, -32, 48], [-2, 16, -331, 2261]),
            (-2, [-3, 9, 2, -1, -48, 2304, 32, -16], [-10, 0, 181, 1685]),
        ];
        for (radius, expected, indices) in fixtures {
            let size = NoiseTileSize::new(radius);
            assert_eq!(
                [
                    size.chunk_length,
                    size.chunk_size,
                    size.chunk_min,
                    size.chunk_max,
                    size.region_length,
                    size.region_size,
                    size.min,
                    size.max,
                ],
                expected
            );
            assert_eq!(
                [
                    size.chunk_index_of(-7, 1),
                    size.chunk_index_of_rel(-1, 1),
                    size.index_of(-11, -4),
                    size.index_of_rel(-11, -4),
                ],
                indices
            );
        }
        assert_eq!(NoiseTileSize::DEFAULT, NoiseTileSize::new(1));
        assert_eq!(
            NoiseTileSize::DEFAULT.to_string(),
            "NoiseTileSize{chunkDiameter=3, chunkSize=9, chunkMin=-1, chunkMax=2, regionDiameter=48, regionSize=2304, min=-16, max=32}"
        );
    }

    fn fixture_map(length: i32) -> Vec<f32> {
        (0..length.wrapping_mul(length))
            .map(|index| {
                (index.wrapping_mul(37) % 101) as f32 / 100.0 + (index / length) as f32 * 0.125
            })
            .collect()
    }

    fn hash(map: &[f32]) -> u64 {
        map.iter().fold(0xcbf29ce484222325, |hash, value| {
            (hash ^ u64::from(value.to_bits())).wrapping_mul(1099511628211)
        })
    }

    #[test]
    fn heightmap_smoothing_matches_current_java_source() {
        let cases = [
            (HeightmapSmoothingSettings::default(), 0x265dc7ee0ba807f3),
            (
                HeightmapSmoothingSettings {
                    iterations: 2,
                    smoothing_radius: 1.75,
                    smoothing_rate: 0.85,
                },
                0x9008debe23dcc046,
            ),
            (
                HeightmapSmoothingSettings {
                    iterations: 1,
                    smoothing_radius: 2.5,
                    smoothing_rate: 0.35,
                },
                0x7eea390865bcc9ab,
            ),
            (
                HeightmapSmoothingSettings {
                    iterations: 3,
                    smoothing_radius: 0.0,
                    smoothing_rate: 1.0,
                },
                0x265dc7ee0ba807f3,
            ),
        ];
        for (settings, expected) in cases {
            let mut map = fixture_map(9);
            HeightmapSmoothing::apply(&mut map, 9, settings);
            assert_eq!(hash(&map), expected);
        }
    }

    fn brush_hash(offsets: &[i32], weights: &[f32]) -> u64 {
        offsets
            .iter()
            .zip(weights)
            .fold(0xcbf29ce484222325, |hash, (&offset, &weight)| {
                let hash = (hash ^ offset as u32 as u64).wrapping_mul(1099511628211);
                (hash ^ u64::from(weight.to_bits())).wrapping_mul(1099511628211)
            })
    }

    #[test]
    fn erosion_brushes_match_current_java_source() {
        let settings = ErosionFilterSettings::default();
        assert_eq!(
            [
                settings.droplets_per_chunk,
                settings.droplet_lifetime,
                settings.droplet_volume.to_bits() as i32,
                settings.droplet_velocity.to_bits() as i32,
                settings.erosion_rate.to_bits() as i32,
                settings.deposit_rate.to_bits() as i32,
            ],
            [350, 25, 0x3f333333, 0x3f333333, 0x3f000000, 0x3f000000]
        );
        let fixtures = [
            (16, 0, 43, 0xc61aeca980831d14_u64),
            (16, 8, 79, 0xbe4f8331a112f8cb_u64),
            (16, 136, 145, 0x0cc571d93d0350d4_u64),
            (16, 255, 43, 0xe8cbdfeee1655677_u64),
            (32, 0, 43, 0xc997b62ae4fd4804_u64),
            (32, 16, 79, 0x6a689175594529db_u64),
            (32, 528, 145, 0xb11badbb28d047b4_u64),
            (32, 1023, 43, 0xd033aa0de6cf8087_u64),
        ];
        for (size, index, expected_count, expected_hash) in fixtures {
            let brushes = ErosionBrushes::new(size);
            let (offsets, weights) = brushes.brush(index);
            assert_eq!(offsets.len(), expected_count);
            assert_eq!(brush_hash(offsets, weights), expected_hash);
        }
    }

    fn erosion_map(length: i32) -> Vec<f32> {
        (0..length.wrapping_mul(length))
            .map(|index| {
                let x = index % length;
                let z = index / length;
                0.2 + (x.wrapping_mul(13).wrapping_add(z.wrapping_mul(7)) % 31) as f32 / 100.0
                    + x as f32 / 1000.0
            })
            .collect()
    }

    #[test]
    fn current_erosion_filter_matches_java_maps() {
        let settings = ErosionFilterSettings {
            droplets_per_chunk: 3,
            droplet_lifetime: 10,
            ..ErosionFilterSettings::default()
        };
        let size = NoiseTileSize::DEFAULT;
        let filter = ErosionFilter::new(size.region_length, settings);
        let mut tile_map = erosion_map(size.region_length);
        filter.apply_tile(
            42,
            -3,
            5,
            size,
            &mut ErosionResource::default(),
            &mut tile_map,
        );
        assert_eq!(hash(&tile_map), 0xb36ecb2fd97ba171);

        let mut region_map = erosion_map(size.region_length);
        filter.apply_region(
            -17,
            8,
            -11,
            size.chunk_length,
            size.region_length,
            &mut ErosionResource::default(),
            &mut region_map,
        );
        assert_eq!(hash(&region_map), 0x293bedd53953eb33);
    }
}
