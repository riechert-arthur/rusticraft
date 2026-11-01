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

use super::terraforged::round;

/// TerraForged's `worldgen.noise.erosion.NoiseTileSize` (newer source 6dd607eb; the 1.18.2-0.3.1 jar's class has the
/// same bytecode): the square of chunks around a centre chunk that `ErodedNoiseGenerator` erodes together, `chunk_radius`
/// chunks on every side. Chunk offsets run from `chunk_min` (inclusive) to `chunk_max` (exclusive), block offsets from
/// `min` to `max`, both relative to the centre chunk's origin. All fields and indices are Java `int` math (wrapping),
/// so a negative or huge radius gives Java's wrapped values, not a panic. Unrelated to Engine's tile `Size`
/// (`terraforged_tile`), which sizes power-of-two tiles plus a border.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NoiseTileSize {
    /// `chunkLength` (`chunkDiameter` in `toString`): `1 + 2 * chunkRadius`.
    pub chunk_length: i32,
    /// `chunkSize`: `chunkLength²`, the number of chunks.
    pub chunk_size: i32,
    /// `chunkMin`: `-chunkRadius`.
    pub chunk_min: i32,
    /// `chunkMax`: `chunkMin + chunkLength`.
    pub chunk_max: i32,
    /// `regionLength` (`regionDiameter`): `chunkLength * 16` blocks.
    pub region_length: i32,
    /// `regionSize`: `regionLength²`, the heightmap's length.
    pub region_size: i32,
    /// `min`: `-16 * chunkRadius`.
    pub min: i32,
    /// `max`: `min + regionLength`.
    pub max: i32,
}

impl NoiseTileSize {
    /// `CHUNK_SIZE`: blocks per chunk side.
    pub const CHUNK_SIZE: i32 = 16;
    /// `DEFAULT`: radius 1 (3x3 chunks).
    pub const DEFAULT: Self = Self::new(1);

    /// `new NoiseTileSize(chunkRadius)`.
    pub const fn new(chunk_radius: i32) -> Self {
        let chunk_length = 1_i32.wrapping_add(chunk_radius.wrapping_mul(2));
        let chunk_size = chunk_length.wrapping_mul(chunk_length);
        let chunk_min = chunk_radius.wrapping_neg();
        let chunk_max = chunk_min.wrapping_add(chunk_length);
        let region_length = chunk_length.wrapping_mul(Self::CHUNK_SIZE);
        let region_size = region_length.wrapping_mul(region_length);
        // Java's `-CHUNK_SIZE * chunkRadius`; the same bits as negating the product.
        let min = Self::CHUNK_SIZE.wrapping_neg().wrapping_mul(chunk_radius);
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

    /// `chunkIndexOf(x, z)`: `z * chunkLength + x` for chunk offsets counted from 0; not bounds-checked.
    pub const fn chunk_index_of(self, x: i32, z: i32) -> i32 {
        z.wrapping_mul(self.chunk_length).wrapping_add(x)
    }

    /// `chunkIndexOfRel(x, z)`: [`Self::chunk_index_of`] for chunk offsets relative to the centre chunk.
    pub const fn chunk_index_of_rel(self, x: i32, z: i32) -> i32 {
        self.chunk_index_of(
            x.wrapping_sub(self.chunk_min),
            z.wrapping_sub(self.chunk_min),
        )
    }

    /// `indexOf(x, z)`: `z * regionLength + x` for block offsets counted from 0; not bounds-checked.
    pub const fn index_of(self, x: i32, z: i32) -> i32 {
        z.wrapping_mul(self.region_length).wrapping_add(x)
    }

    /// `indexOfRel(x, z)`: [`Self::index_of`] for block offsets relative to the centre chunk's origin.
    pub const fn index_of_rel(self, x: i32, z: i32) -> i32 {
        self.index_of(x.wrapping_sub(self.min), z.wrapping_sub(self.min))
    }
}

/// Java's `toString` (`chunkDiameter`/`regionDiameter` name the length fields).
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

// Adapted from TerraForged Reimagined (https://github.com/Romoslayer/TerraForged-Reimagined, a24d450):
// `src/main/java/com/terraforged/mod/worldgen/noise/erosion/HeightmapSmoothing.java` and `TerraSettings.Smoothing` in
// `src/main/java/com/terraforged/mod/worldgen/settings/TerraSettings.java`. MIT License, Copyright (c) 2020 TerraForged,
// Copyright (c) 2026 Romoslayer; see `LICENSE-TERRAFORGED-REIMAGINED`.

/// TerraForged Reimagined 26.3's `TerraSettings.Smoothing` (the Filters page's smoothing settings): Reimagined's own
/// settings, not Engine's `FilterSettings.Smoothing` (1 iteration, radius 1.8, rate 0.9) that `terraforged_filters`'
/// `SmoothingSettings` mirrors. Off by default (0 iterations), as TerraForged 0.3.x never smoothed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HeightmapSmoothingSettings {
    /// `iterations`: passes over the map; 0 or less turns smoothing off.
    pub iterations: i32,
    /// `smoothingRadius`: the neighbourhood radius in cells (rounded up for the scan, squared for the weights).
    pub smoothing_radius: f32,
    /// `smoothingRate`: how far each pass moves a cell towards its neighbourhood mean (0 or less turns it off).
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

/// TerraForged Reimagined 26.3's `worldgen.noise.erosion.HeightmapSmoothing` (Romoslayer/TerraForged-Reimagined
/// a24d450): neither the 1.18.2-0.3.1 jar nor the newer source (6dd607eb) has it. Reimagined's
/// `ErodedNoiseGenerator.buildRegion` runs it on the eroded region heightmap, after erosion. It is Engine's `Smoothing`
/// filter (`terraforged_filters::Smoothing`) on a bare height array: the same radius rounding, `1 - d² / r²` weights
/// and in-place z-then-x scan, but without the per-cell erosion mask, the altitude modifier (the heightmap carries no
/// terrain data, so every cell smooths evenly), or absent cells (every neighbour is in the array).
pub struct HeightmapSmoothing;

impl HeightmapSmoothing {
    /// `HeightmapSmoothing.apply(map, length, settings)`: `settings.iterations` passes over the `length × length` map
    /// (row-major, z rows), each moving every cell at least `radius` cells from the edge, in place, towards the
    /// weighted mean of its neighbours within `smoothing_radius` (already-smoothed cells included). Java `int` and
    /// `float` math throughout, so NaN and infinite heights or settings give Java's bits. Panics where Java throws
    /// `ArrayIndexOutOfBoundsException`: when `map` is shorter than `length²` and a read leaves it.
    pub fn apply(map: &mut [f32], length: i32, settings: HeightmapSmoothingSettings) {
        let mut iterations = settings.iterations;
        if iterations <= 0 {
            return;
        }
        let radius_value = settings.smoothing_radius;
        let radius = round(radius_value + 0.5);
        let radius_squared = radius_value * radius_value;
        let strength = settings.smoothing_rate;
        // Java's `<=` tests: a NaN radius rounds to 0 and returns, a NaN rate does not (it smooths to NaN).
        if radius <= 0 || radius_squared <= 0.0 || strength <= 0.0 {
            return;
        }
        let end = length.wrapping_sub(radius);
        while iterations > 0 {
            for z in radius..end {
                for x in radius..end {
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

/// The hydraulic erosion settings of TerraForged Reimagined 26.3's `ErodedNoiseGenerator`
/// (`TerraSettings.Erosion`, copied field by field into the `FilterSettings.Erosion` its
/// `ErosionFilter` reads): 350 droplets per chunk, lifetime 25, volume and velocity 0.7, erosion
/// and deposit rates 0.5. The 1.18.2-0.3.1 jar and 6dd607eb build Engine's `FilterSettings.Erosion`
/// (`terraforged_filters::ErosionSettings`) and set only the 350 droplets, so their lifetime is
/// Engine's 12. Adapted from TerraForged Reimagined (MIT, see `LICENSE-TERRAFORGED-REIMAGINED`).
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

/// Reimagined's `ErosionFilter.Disc`: the unclipped brush (every cell with `dx² + dy² < r²`, rows
/// then columns), its raw `1 - d / r` weights, their f32 sum in that order, and its reach (6 at
/// radius 7).
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

/// The radius-7 erosion brushes of TerraForged's `ErosionFilter` (the mod's hydraulic erosion), in
/// TerraForged Reimagined 26.3's layout (Romoslayer/TerraForged-Reimagined a24d450, MIT, see
/// `LICENSE-TERRAFORGED-REIMAGINED`): one normalized disc of offsets relative to the droplet's cell,
/// shared by every interior cell, plus a clipped, re-weighted copy for each cell within the disc's
/// reach of an edge. Every cell gets the same points, order and weight bits as the 1.18.2-0.3.1
/// jar's and 6dd607eb's per-cell absolute tables, which are Engine's `Erosion.initBrushes` at
/// radius 7 (`terraforged_filters::ErosionBrushes`, flat). It stays a separate type because that
/// layout grows with the map's area: ~110 MB for Reimagined's 256x256 region map, against ~6 MB here.
pub struct ErosionBrushes {
    offsets: Box<[i32]>,
    weights: Box<[f32]>,
    /// Java's `edgeOffsets[i]`/`edgeWeights[i]` (both null or both set): one slot per cell.
    edges: Box<[Option<Box<EdgeBrush>>]>,
}

/// A clipped, re-weighted brush of a cell near the map's edge.
struct EdgeBrush {
    offsets: Box<[i32]>,
    weights: Box<[f32]>,
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
    /// `ErosionFilter.erosionRadius`.
    pub const RADIUS: i32 = 7;

    /// `new ErosionFilter(mapSize, ..)`'s brush tables, in Java `int` math: a negative `map_size`
    /// whose square is positive gives empty brushes, as in Java. Panics where Java throws (the
    /// square wraps negative).
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
        // Java's `new int[mapSize * mapSize][]`: the square wraps, and a negative one throws.
        let length = usize::try_from(map_size.wrapping_mul(map_size))
            .expect("ErosionFilter's map size squared must not wrap negative (Java throws)");
        let mut brushes = Self {
            offsets,
            weights,
            edges: std::iter::repeat_with(|| None).take(length).collect(),
        };
        brushes.init_edges(map_size, &disc);
        brushes
    }

    /// The offsets (added to `index`) and weights `applyDrop` erodes through for a droplet in cell
    /// `index`. Panics past `map_size²` cells, where Java throws.
    pub fn brush(&self, index: usize) -> (&[i32], &[f32]) {
        match &self.edges[index] {
            Some(edge) => (&edge.offsets, &edge.weights),
            None => (&self.offsets, &self.weights),
        }
    }

    fn init_edges(&mut self, size: i32, disc: &ErosionDisc) {
        for index in 0..self.edges.len() {
            let center_x = index as i32 % size;
            let center_y = index as i32 / size;
            let clipped = center_x < disc.reach
                || center_x >= size - disc.reach
                || center_y < disc.reach
                || center_y >= size - disc.reach;
            if !clipped {
                continue;
            }
            let mut count = 0;
            let mut weight_sum = 0.0_f32;
            for point in 0..disc.weights.len() {
                let x = center_x.wrapping_add(disc.x[point]);
                let y = center_y.wrapping_add(disc.y[point]);
                if x >= 0 && x < size && y >= 0 && y < size {
                    weight_sum += disc.weights[point];
                    count += 1;
                }
            }
            let mut offsets = Vec::with_capacity(count);
            let mut weights = Vec::with_capacity(count);
            for point in 0..disc.weights.len() {
                let x = center_x.wrapping_add(disc.x[point]);
                let y = center_y.wrapping_add(disc.y[point]);
                if x >= 0 && x < size && y >= 0 && y < size {
                    offsets.push(disc.y[point].wrapping_mul(size).wrapping_add(disc.x[point]));
                    weights.push(disc.weights[point] / weight_sum);
                }
            }
            self.edges[index] = Some(Box::new(EdgeBrush {
                offsets: offsets.into_boxed_slice(),
                weights: weights.into_boxed_slice(),
            }));
        }
    }
}

/// `pub(crate)`: noise_generator.rs's tests smooth real `NoiseData` heights with these helpers.
#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::fmt::Write;

    const RADII: [i32; 16] = [
        0,
        1,
        2,
        3,
        -1,
        -2,
        7,
        1000,
        -1000,
        46340,
        1 << 26,
        1 << 30,
        134_217_727,
        i32::MAX,
        i32::MIN,
        -(1 << 30) - 1,
    ];
    const COORDS: [i32; 16] = [
        0,
        1,
        -1,
        15,
        16,
        -16,
        -17,
        47,
        48,
        -48,
        1000,
        -1000,
        1 << 27,
        1 << 30,
        i32::MAX,
        i32::MIN,
    ];
    const LOOP_RADII: [i32; 6] = [0, 1, 2, 3, -1, -2];
    const CHUNKS: [(i32, i32); 5] = [
        (0, 0),
        (-1, 2),
        (-3, -5),
        (134_217_727, -134_217_728),
        (i32::MAX, i32::MIN),
    ];

    /// The oracle's FNV-1a 64 over Java `int`s (low 32 bits, unsigned) and its value count.
    struct Fold(u64, i32);

    impl Fold {
        fn new() -> Self {
            Self(0xcbf2_9ce4_8422_2325, 0)
        }

        fn put(&mut self, value: i32) {
            self.0 = (self.0 ^ u64::from(value as u32)).wrapping_mul(0x0100_0000_01b3);
            self.1 += 1;
        }
    }

    /// NoiseTileSizeOracle (`.local/terraforged/oracles/noise_generator/run-tile-size.sh`): the real class of the
    /// newer source (6dd607eb) and of the jar, interpreted and JIT, all give this text. Its `e` lines replay
    /// `ErodedNoiseGenerator`'s chunk-to-tile loops (`collectNeighbours`, `generateCenterChunk` over `NoiseData`'s
    /// -1..17, `awaitNeighbours`) with Java `int` math, including negative radii (empty loops) and wrapping chunks.
    #[test]
    fn noise_tile_size_matches_current_java_source() {
        let mut out = String::new();
        for radius in RADII {
            let s = NoiseTileSize::new(radius);
            writeln!(
                out,
                "s {radius} {} {} {} {} {} {} {} {} | {s}",
                s.chunk_length,
                s.chunk_size,
                s.chunk_min,
                s.chunk_max,
                s.region_length,
                s.region_size,
                s.min,
                s.max
            )
            .unwrap();
            let mut fold = Fold::new();
            for z in COORDS {
                for x in COORDS {
                    fold.put(s.chunk_index_of(x, z));
                    fold.put(s.chunk_index_of_rel(x, z));
                    fold.put(s.index_of(x, z));
                    fold.put(s.index_of_rel(x, z));
                }
            }
            writeln!(out, "i {radius} {:x}", fold.0).unwrap();
        }
        writeln!(
            out,
            "d {} {}",
            NoiseTileSize::DEFAULT,
            NoiseTileSize::CHUNK_SIZE
        )
        .unwrap();
        for radius in LOOP_RADII {
            let tile_size = NoiseTileSize::new(radius);
            for (chunk_x, chunk_z) in CHUNKS {
                let mut fold = Fold::new();
                for dz in tile_size.chunk_min..tile_size.chunk_max {
                    for dx in tile_size.chunk_min..tile_size.chunk_max {
                        if dx == 0 && dz == 0 {
                            continue;
                        }
                        fold.put(tile_size.chunk_index_of_rel(dx, dz));
                        fold.put(chunk_x.wrapping_add(dx));
                        fold.put(chunk_z.wrapping_add(dz));
                    }
                }
                let start_x = chunk_x.wrapping_shl(4);
                let start_z = chunk_z.wrapping_shl(4);
                for dz in -1..17 {
                    fold.put(start_z.wrapping_add(dz));
                    for dx in -1..17 {
                        fold.put(start_x.wrapping_add(dx));
                        fold.put(tile_size.index_of_rel(dx, dz));
                    }
                }
                for cz in tile_size.chunk_min..tile_size.chunk_max {
                    for cx in tile_size.chunk_min..tile_size.chunk_max {
                        if cx == 0 && cz == 0 {
                            continue;
                        }
                        fold.put(tile_size.chunk_index_of_rel(cx, cz));
                        let (rel_x, rel_z) = (cx.wrapping_shl(4), cz.wrapping_shl(4));
                        for i in 0..256 {
                            let (dx, dz) = (i & 15, i >> 4);
                            fold.put(
                                tile_size
                                    .index_of_rel(rel_x.wrapping_add(dx), rel_z.wrapping_add(dz)),
                            );
                        }
                    }
                }
                writeln!(
                    out,
                    "e {radius} {chunk_x} {chunk_z} {} {:x}",
                    fold.1, fold.0
                )
                .unwrap();
            }
        }
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 63);
        assert_eq!(
            lines[2],
            "s 1 3 9 -1 2 48 2304 -16 32 | NoiseTileSize{chunkDiameter=3, chunkSize=9, chunkMin=-1, chunkMax=2, regionDiameter=48, regionSize=2304, min=-16, max=32}"
        );
        assert_eq!(
            lines[22],
            "s 1073741824 -2147483647 1 -1073741824 1073741825 16 256 0 16 | NoiseTileSize{chunkDiameter=-2147483647, chunkSize=1, chunkMin=-1073741824, chunkMax=1073741825, regionDiameter=16, regionSize=256, min=0, max=16}"
        );
        assert_eq!(lines[38], "e 1 0 0 2746 6552469816fd2fae");
        assert_eq!(
            lines[62],
            "e -2 2147483647 -2147483648 666 f0b80a713d5ded92"
        );
        let fnv = out.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
        });
        assert_eq!(fnv, 0xe62f_61b7_b3c5_c62a);
        assert_eq!(NoiseTileSize::DEFAULT, NoiseTileSize::new(1));
    }

    /// `HeightmapSmoothingOracle.SETTINGS` (iterations, radius, rate); row 0 is the defaults.
    pub(crate) const SMOOTHING_SETTINGS: [(i32, f32, f32); 24] = [
        (0, 0.0, 0.0),
        (1, 1.75, 0.85),
        (2, 1.75, 0.85),
        (3, 1.75, 0.85),
        (1, 1.8, 0.9),
        (1, 2.5, 0.35),
        (2, 5.0, 1.0),
        (1, 0.4, 0.85),
        (1, 0.5, 0.85),
        (1, 1.0, 0.5),
        (1, 1.75, 2.0),
        (-1, 1.75, 0.85),
        (3, 0.0, 1.0),
        (1, -1.0, 0.85),
        (1, -0.6, 0.85),
        (1, 1.75, 0.0),
        (1, 1.75, -0.5),
        (1, f32::NAN, 0.85),
        (1, 1.75, f32::NAN),
        (1, f32::INFINITY, 0.85),
        (1, f32::NEG_INFINITY, 0.85),
        (1, f32::from_bits(1), 0.85),
        (1, 1e-20, 0.85),
        (1, 3e19, 0.85),
    ];

    pub(crate) fn smoothing_settings(row: usize) -> HeightmapSmoothingSettings {
        if row == 0 {
            return HeightmapSmoothingSettings::default();
        }
        let (iterations, smoothing_radius, smoothing_rate) = SMOOTHING_SETTINGS[row];
        HeightmapSmoothingSettings {
            iterations,
            smoothing_radius,
            smoothing_rate,
        }
    }

    /// FNV-1a over `floatToIntBits` (NaN canonical), as the oracle.
    pub(crate) fn smoothing_hash(map: &[f32]) -> u64 {
        map.iter().fold(0xcbf2_9ce4_8422_2325, |hash, value| {
            let bits = if value.is_nan() {
                0x7fc0_0000
            } else {
                value.to_bits()
            };
            (hash ^ u64::from(bits)).wrapping_mul(0x100_0000_01b3)
        })
    }

    /// Smooths `map` and folds its input and output hashes into `digest` (the oracle's `row`/`realrow` digests).
    pub(crate) fn smooth_and_digest(digest: &mut u64, map: &mut [f32], length: i32, row: usize) {
        let input = smoothing_hash(map);
        HeightmapSmoothing::apply(map, length, smoothing_settings(row));
        for hash in [input, smoothing_hash(map)] {
            *digest = (*digest ^ hash).wrapping_mul(0x100_0000_01b3);
        }
    }

    /// `HeightmapSmoothingOracle.map`: 0 smooth ramps, 1 xorshift values in [-2, 2), 2 those with NaN, ±infinity,
    /// -0 and the smallest subnormal mixed in.
    fn smoothing_map(pattern: i32, length: i32) -> Vec<f32> {
        let mut random = (0x9e37_79b9_u32 as i32)
            .wrapping_add(pattern * 31)
            .wrapping_add(length);
        (0..length * length)
            .map(|i| {
                random ^= random << 13;
                random ^= ((random as u32) >> 17) as i32;
                random ^= random << 5;
                let value = ((random as u32) >> 8) as f32 * (1.0 / 4_194_304.0) - 2.0;
                match pattern {
                    0 => (i * 37 % 101) as f32 / 100.0 + (i / length) as f32 * 0.125,
                    1 => value,
                    _ if i % 7 == 3 => f32::NAN,
                    _ if i % 11 == 5 => f32::INFINITY,
                    _ if i % 13 == 6 => -0.0,
                    _ if i % 17 == 2 => f32::from_bits(1),
                    _ if i % 19 == 9 => f32::NEG_INFINITY,
                    _ => value,
                }
            })
            .collect()
    }

    /// `HeightmapSmoothingOracle.java` (`.local/terraforged/oracles/noise_generator/run-smoothing.sh`): Reimagined
    /// 26.3's own `HeightmapSmoothing` and `TerraSettings.Smoothing` compiled from its source. One digest per settings
    /// row over 3 map patterns × 8 lengths (0 to 256, Reimagined's `MAP_SIZE`); the oracle's `case` lines hold each map.
    const EXPECTED_SMOOTHING: [&str; 25] = [
        "defaults 0 3fe00000 3f59999a",
        "row 0 bed4f63f964064e7",
        "row 1 276d2ceeeccf2049",
        "row 2 fb486916b62f6dbb",
        "row 3 cf1acfa80e3aa4e1",
        "row 4 f6a84102166c941d",
        "row 5 ecfad454952ece95",
        "row 6 6330770aca045c49",
        "row 7 1a36bd105a8064e7",
        "row 8 1a36bd105a8064e7",
        "row 9 bd254a8aaafb71d",
        "row 10 c7795eb6e0bb30c2",
        "row 11 bed4f63f964064e7",
        "row 12 bed4f63f964064e7",
        "row 13 bed4f63f964064e7",
        "row 14 bed4f63f964064e7",
        "row 15 bed4f63f964064e7",
        "row 16 bed4f63f964064e7",
        "row 17 bed4f63f964064e7",
        "row 18 561c5a76bc4ed6e3",
        "row 19 bed4f63f964064e7",
        "row 20 bed4f63f964064e7",
        "row 21 bed4f63f964064e7",
        "row 22 1a36bd105a8064e7",
        "row 23 bed4f63f964064e7",
    ];

    #[test]
    fn heightmap_smoothing_matches_reimagined_java() {
        let defaults = HeightmapSmoothingSettings::default();
        let mut lines = vec![format!(
            "defaults {} {:x} {:x}",
            defaults.iterations,
            defaults.smoothing_radius.to_bits(),
            defaults.smoothing_rate.to_bits()
        )];
        for row in 0..SMOOTHING_SETTINGS.len() {
            let mut digest = 0xcbf2_9ce4_8422_2325;
            for pattern in 0..3 {
                for length in [0, 1, 3, 5, 9, 18, 33, 256] {
                    let mut map = smoothing_map(pattern, length);
                    smooth_and_digest(&mut digest, &mut map, length, row);
                }
            }
            lines.push(format!("row {row} {digest:x}"));
        }
        assert_eq!(lines, EXPECTED_SMOOTHING);
    }

    /// In place: a pass reads the cells it already moved (the z-then-x scan order matters), and only cells `radius`
    /// from the edge move.
    #[test]
    fn heightmap_smoothing_is_in_place_and_keeps_the_border() {
        let settings = smoothing_settings(1);
        let mut map = smoothing_map(1, 9);
        let original = map.clone();
        HeightmapSmoothing::apply(&mut map, 9, settings);
        // Radius 1.75 scans 2 cells (round(2.25)): rows and columns 0, 1, 7, 8 stay.
        for z in 0..9 {
            for x in 0..9 {
                let index = z * 9 + x;
                let inner = (2..7).contains(&x) && (2..7).contains(&z);
                assert_eq!(map[index] != original[index], inner, "{x} {z}");
            }
        }
        // The same pass over an untouched copy of every neighbourhood (a two-buffer filter) differs.
        let mut copied = original.clone();
        for z in 2..7_i32 {
            for x in 2..7_i32 {
                let (mut total, mut weights) = (0.0_f32, 0.0_f32);
                for dz in -2..=2_i32 {
                    for dx in -2..=2_i32 {
                        let d = (dx * dx + dz * dz) as f32;
                        if d > 1.75 * 1.75 {
                            continue;
                        }
                        let weight = 1.0 - d / (1.75 * 1.75);
                        total += original[((z + dz) * 9 + x + dx) as usize] * weight;
                        weights += weight;
                    }
                }
                let index = (z * 9 + x) as usize;
                copied[index] -= (copied[index] - total / weights) * 0.85;
            }
        }
        assert_ne!(smoothing_hash(&copied), smoothing_hash(&map));
    }

    /// Java throws `ArrayIndexOutOfBoundsException` once a read leaves a map shorter than `length²`.
    #[test]
    fn heightmap_smoothing_panics_on_a_short_map() {
        let result = std::panic::catch_unwind(|| {
            HeightmapSmoothing::apply(&mut [0.0; 10], 5, smoothing_settings(1));
        });
        assert!(result.is_err());
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
    fn erosion_settings_and_cell_brushes_match_reimagined_java() {
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

    /// FNV-1a over every cell's absolute brush (point count, then `index + offset` and the raw
    /// weight bits), as `ErosionBrushOracle` folds it.
    fn absolute_brush_digest(brushes: &ErosionBrushes, length: usize) -> (u64, usize) {
        let fold = |hash: u64, value: i32| (hash ^ value as u32 as u64).wrapping_mul(1099511628211);
        let mut digest = 0xcbf29ce484222325;
        let mut points = 0;
        for index in 0..length {
            let (offsets, weights) = brushes.brush(index);
            digest = fold(digest, offsets.len() as i32);
            for (&offset, &weight) in offsets.iter().zip(weights) {
                digest = fold(digest, (index as i32).wrapping_add(offset));
                digest = fold(digest, weight.to_bits() as i32);
            }
            points += offsets.len();
        }
        (digest, points)
    }

    /// `.local/terraforged/oracles/erosion_brushes/run.sh` builds Reimagined's own `ErosionFilter`
    /// (and 6dd607eb's and the 1.18.2-0.3.1 jar's, which give the same brush for every cell) and
    /// digests every cell's brush for these sizes, negative and wrapping ones included.
    #[test]
    fn erosion_brushes_match_reimagined_java() {
        let fixtures = [
            (-16, 256, 0, 0xd80ac658736bb725_u64),
            (-1, 1, 0, 0xaf63bd4c8601b7df_u64),
            (65536, 0, 0, 0xcbf29ce484222325_u64),
            (0, 0, 0, 0xcbf29ce484222325_u64),
            (1, 1, 1, 0x50a396d1e2a7310c_u64),
            (2, 4, 16, 0xbbf76556bd76f085_u64),
            (3, 9, 81, 0x4cceeed5e7ff1400_u64),
            (5, 25, 625, 0x0d68d8b1e2a60f65_u64),
            (7, 49, 2341, 0xa1b907e6c24cfe17_u64),
            (12, 144, 11936, 0x215955d284d71a81_u64),
            (13, 169, 14725, 0xf4f7e58a4c3df30d_u64),
            (14, 196, 17804, 0xeb7e45ce751c3913_u64),
            (15, 225, 21173, 0x795d614d50411829_u64),
            (16, 256, 24832, 0xcf952a5812f2fb45_u64),
            (17, 289, 28781, 0x3d531d7a5c65c321_u64),
            (20, 400, 42368, 0x1afd2431e90b9329_u64),
            (31, 961, 114517, 0xf06a6e9b3f0eb559_u64),
            (32, 1024, 122816, 0x37a272f77f8d4625_u64),
            (80, 6400, 862208, 0xdcf80d9e03c14145_u64),
            (144, 20736, 2887424, 0x23fb9c9798624345_u64),
            (256, 65536, 9289792, 0x984e42ceb4f41025_u64),
        ];
        for (size, cells, expected_points, expected_digest) in fixtures {
            let brushes = ErosionBrushes::new(size);
            let (digest, points) = absolute_brush_digest(&brushes, cells);
            assert_eq!(
                (points, digest),
                (expected_points, expected_digest),
                "size {size}"
            );
        }
    }

    /// Reimagined only changes how the brushes are stored: every cell's points, order and weight
    /// bits are those of Engine's `Erosion.initBrushes` at radius 7 (the same code as 6dd607eb's and
    /// the jar's `ErosionFilter`), ported as `terraforged_filters::ErosionBrushes`.
    #[test]
    fn erosion_brushes_match_engine_brushes_at_radius_seven() {
        for size in [1, 2, 7, 13, 14, 15, 16, 17, 20, 31, 32, 80] {
            let brushes = ErosionBrushes::new(size);
            let engine = super::super::terraforged_filters::ErosionBrushes::new(
                size as usize,
                ErosionBrushes::RADIUS,
            );
            for index in 0..(size * size) as usize {
                let (offsets, weights) = brushes.brush(index);
                let (indices, engine_weights) = engine.get(index);
                let absolute: Vec<usize> = offsets
                    .iter()
                    .map(|&offset| (index as i32 + offset) as usize)
                    .collect();
                assert_eq!(absolute, indices, "size {size} cell {index}");
                let bits = |w: &[f32]| w.iter().map(|w| w.to_bits()).collect::<Vec<_>>();
                assert_eq!(
                    bits(weights),
                    bits(engine_weights),
                    "size {size} cell {index}"
                );
            }
        }
    }

    #[test]
    fn erosion_brushes_are_shareable_across_threads() {
        fn shareable<T: Send + Sync>() {}
        shareable::<ErosionBrushes>();
    }

    #[test]
    #[should_panic(expected = "must not wrap negative")]
    fn erosion_brushes_panic_where_java_throws() {
        // 46341² wraps to a negative `int`: Java's array allocation throws.
        ErosionBrushes::new(46341);
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
