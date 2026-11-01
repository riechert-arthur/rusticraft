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
    terraforged::{
        Domain, Interpolation, Module, Source, SourceConfig, SourceKind, cell_vector, floor,
        noise_map, value_coord_2d,
    },
    terraforged_engine::{Cell, ControlPoints, Levels, RiverSettings, Seed, WorldSettings},
    terraforged_populator::Populator,
    terraforged_river::{RiverCache, Rivermap, SimpleRiverGenerator},
    terraforged_util::{pack, unpack_left, unpack_right},
};

pub struct AdvancedContinentGenerator {
    seed: i32,
    skipping_seed: i32,
    continent_scale: i32,
    jitter: f32,
    has_skipping: bool,
    skip_threshold: f32,
    control_points: ControlPoints,
    frequency: f32,
    variance: f32,
    variance_seed: i32,
    warp: Domain,
    cliff_noise: Module,
    bay_noise: Module,
    river_cache: RiverCache,
    river_root_seed: i32,
    world: WorldSettings,
    rivers: RiverSettings,
    levels: Levels,
}

impl AdvancedContinentGenerator {
    pub fn new(seed: &mut Seed, settings: WorldSettings) -> Self {
        Self::with_rivers(
            seed,
            settings,
            RiverSettings::default(),
            Levels::from_properties(settings.properties),
        )
    }

    pub fn with_rivers(
        seed: &mut Seed,
        settings: WorldSettings,
        rivers: RiverSettings,
        levels: Levels,
    ) -> Self {
        let river_root_seed = seed.root();
        let generator_seed = seed.next();
        let skipping_seed = seed.next();
        let continent_scale = settings.continent.continent_scale;
        let tectonic_scale = continent_scale.wrapping_mul(4);
        let frequency = 1.0 / tectonic_scale as f32;
        let variance_seed = seed.next();
        let warp_scale = noise_round(tectonic_scale as f32 * 0.225);
        let strength = noise_round(tectonic_scale as f32 * 0.33) as f32;
        let warp = Domain::domain_warp(
            source(
                SourceKind::Perlin2,
                seed.next(),
                warp_scale,
                settings.continent.continent_noise_octaves as usize,
                settings.continent.continent_noise_gain,
                settings.continent.continent_noise_lacunarity,
            ),
            source(
                SourceKind::Perlin2,
                seed.next(),
                warp_scale,
                settings.continent.continent_noise_octaves as usize,
                settings.continent.continent_noise_gain,
                settings.continent.continent_noise_lacunarity,
            ),
            Module::constant(strength),
        );
        let frequency_scale = Module::constant(1.0 / frequency);
        let cliff_noise = source(
            SourceKind::Simplex2,
            seed.next(),
            continent_scale / 2,
            2,
            0.5,
            2.0,
        )
        .clamp(Module::constant(0.1), Module::constant(0.25))
        .map(Module::constant(0.0), Module::constant(1.0))
        .frequency(frequency_scale.clone(), frequency_scale.clone());
        let bay_noise = source(SourceKind::Simplex, seed.next(), 100, 1, 0.5, 2.0)
            .scale(Module::constant(0.1))
            .bias(Module::constant(0.9))
            .frequency(frequency_scale.clone(), frequency_scale);
        Self {
            seed: generator_seed,
            skipping_seed,
            continent_scale,
            jitter: settings.continent.continent_jitter,
            has_skipping: settings.continent.continent_skipping > 0.0,
            skip_threshold: settings.continent.continent_skipping,
            control_points: ControlPoints::new(settings.control_points),
            frequency,
            variance: settings.continent.continent_size_variance,
            variance_seed,
            warp,
            cliff_noise,
            bay_noise,
            river_cache: RiverCache::new(),
            river_root_seed,
            world: settings,
            rivers,
            levels,
        }
    }

    pub fn rivermap(&self, x: i32, z: i32) -> std::sync::Arc<Rivermap> {
        self.river_cache.get_rivers(x, z, |x, z, id| {
            SimpleRiverGenerator::new(
                self,
                self.river_root_seed,
                self.world,
                self.rivers,
                self.levels,
            )
            .generate_rivers(x, z, id)
        })
    }

    pub fn edge_value(&self, x: f32, z: f32) -> f32 {
        let mut cell = Cell::default();
        self.apply(&mut cell, x, z);
        cell.continent_edge
    }

    pub fn land_value(&self, x: f32, z: f32) -> f32 {
        self.edge_value(x, z)
    }

    pub fn nearest_center(&self, x: f32, z: f32) -> i64 {
        let mut cell = Cell::default();
        self.apply(&mut cell, x, z);
        pack(cell.continent_x, cell.continent_z)
    }

    pub fn distance_to_ocean(&self, center_x: i32, center_z: i32, dx: f32, dz: f32) -> f32 {
        let mut high = self.distance_to_edge(center_x, center_z, dx, dz);
        let mut low = 0.0;
        for _ in 0..50 {
            let mid = (low + high) / 2.0;
            let x = center_x as f32 + dx * mid;
            let z = center_z as f32 + dz * mid;
            if self.edge_value(x, z) > self.control_points.shallow_ocean {
                low = mid;
            } else {
                high = mid;
            }
            if high - low < 10.0 {
                break;
            }
        }
        high
    }

    pub fn distance_to_edge(&self, center_x: i32, center_z: i32, dx: f32, dz: f32) -> f32 {
        let mut distance = self.continent_scale.wrapping_mul(4) as f32;
        for _ in 0..10 {
            let x = center_x as f32 + dx * distance;
            let z = center_z as f32 + dz * distance;
            let center = self.nearest_center(x, z);
            distance += distance;
            if unpack_left(center) == center_x && unpack_right(center) == center_z {
                continue;
            }
            let mut low = 0.0;
            let mut high = distance;
            for _ in 0..50 {
                let mid = (low + high) / 2.0;
                let x = center_x as f32 + dx * mid;
                let z = center_z as f32 + dz * mid;
                let center = self.nearest_center(x, z);
                if unpack_left(center) == center_x && unpack_right(center) == center_z {
                    low = mid;
                } else {
                    high = mid;
                }
                if high - low < 50.0 {
                    break;
                }
            }
            return high;
        }
        distance
    }

    fn should_skip(&self, cell_x: i32, cell_y: i32) -> bool {
        self.has_skipping
            && (cell_x != 0 || cell_y != 0)
            && cell_value(self.skipping_seed, cell_x, cell_y) < self.skip_threshold
    }

    fn distance_value(&self, x: f32, y: f32, cell_x: i32, cell_y: i32, mut distance: f32) -> f32 {
        if self.variance > 0.0 && (cell_x != 0 || cell_y != 0) {
            let size_value = cell_value(self.variance_seed, cell_x, cell_y);
            distance *= noise_map(size_value, 0.0, self.variance, self.variance);
        }
        distance = f64::from(distance).sqrt() as f32;
        distance = noise_map(distance, 0.05, 0.25, 0.2);
        distance = self.coastal_distance_value(x, y, distance);
        if distance < self.control_points.inland && distance >= self.control_points.shallow_ocean {
            distance = self.coastal_distance_value(x, y, distance);
        }
        distance
    }

    fn coastal_distance_value(&self, x: f32, y: f32, mut distance: f32) -> f32 {
        if distance > self.control_points.shallow_ocean && distance < self.control_points.inland {
            let alpha = distance / self.control_points.inland;
            let cliff = self.cliff_noise.value(x, y);
            distance = distance * cliff + alpha * (distance - distance * cliff);
            if distance < self.control_points.shallow_ocean {
                distance = self.control_points.shallow_ocean * self.bay_noise.value(x, y);
            }
        }
        distance
    }

    fn corrected_center(&self, point: f32, average: f32) -> i32 {
        let point = (point + 0.35 * (average - point)) / self.frequency;
        point as i32
    }
}

impl Populator for AdvancedContinentGenerator {
    fn apply(&self, cell: &mut Cell, x: f32, y: f32) {
        let point_x = self.warp.x(x, y) * self.frequency;
        let point_y = self.warp.y(x, y) * self.frequency;
        let x_index = floor(point_x);
        let y_index = floor(point_y);
        let mut cell_x = x_index;
        let mut cell_y = y_index;
        let mut cell_point_x = point_x;
        let mut cell_point_y = point_y;
        let mut nearest = f32::MAX;
        for candidate_y in y_index.wrapping_sub(1)..=y_index.wrapping_add(1) {
            for candidate_x in x_index.wrapping_sub(1)..=x_index.wrapping_add(1) {
                let vector = cell_vector(self.seed, candidate_x, candidate_y);
                let x = candidate_x as f32 + vector[0] * self.jitter;
                let y = candidate_y as f32 + vector[1] * self.jitter;
                let distance = line_distance2(point_x, point_y, x, y);
                if distance < nearest {
                    cell_point_x = x;
                    cell_point_y = y;
                    cell_x = candidate_x;
                    cell_y = candidate_y;
                    nearest = distance;
                }
            }
        }
        nearest = f32::MAX;
        let mut sum_x = 0.0;
        let mut sum_y = 0.0;
        for candidate_y in cell_y.wrapping_sub(1)..=cell_y.wrapping_add(1) {
            for candidate_x in cell_x.wrapping_sub(1)..=cell_x.wrapping_add(1) {
                if candidate_x == cell_x && candidate_y == cell_y {
                    continue;
                }
                let vector = cell_vector(self.seed, candidate_x, candidate_y);
                let x = candidate_x as f32 + vector[0] * self.jitter;
                let y = candidate_y as f32 + vector[1] * self.jitter;
                let distance =
                    bisector_distance2(point_x, point_y, cell_point_x, cell_point_y, x, y);
                sum_x += x;
                sum_y += y;
                if distance < nearest {
                    nearest = distance;
                }
            }
        }
        if self.should_skip(cell_x, cell_y) {
            return;
        }
        cell.continent_id = cell_value(self.seed, cell_x, cell_y);
        cell.continent_edge = self.distance_value(point_x, point_y, cell_x, cell_y, nearest);
        cell.continent_x = self.corrected_center(cell_point_x, sum_x / 8.0);
        cell.continent_z = self.corrected_center(cell_point_y, sum_y / 8.0);
    }

    fn value(&self, x: f32, z: f32) -> f32 {
        self.edge_value(x, z)
    }
}

fn source(
    kind: SourceKind,
    seed: i32,
    scale: i32,
    octaves: usize,
    gain: f32,
    lacunarity: f32,
) -> Module {
    Module::source(Source::new(
        kind,
        SourceConfig {
            seed,
            octaves,
            gain,
            frequency: 1.0 / scale as f32,
            lacunarity,
            interpolation: Interpolation::Curve3,
        },
    ))
}

fn noise_round(value: f32) -> i32 {
    if value >= 0.0 {
        (value + 0.5) as i32
    } else {
        (value - 0.5) as i32
    }
}

fn cell_value(seed: i32, cell_x: i32, cell_y: i32) -> f32 {
    0.5 + value_coord_2d(seed, cell_x, cell_y) * 0.5
}

fn line_distance2(x1: f32, y1: f32, x2: f32, y2: f32) -> f32 {
    let dx = x2 - x1;
    let dy = y2 - y1;
    dx * dx + dy * dy
}

fn bisector_distance2(x: f32, y: f32, ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let midpoint_x = (ax + bx) * 0.5;
    let midpoint_y = (ay + by) * 0.5;
    let dx = bx - ax;
    let dy = by - ay;
    distance2_line(
        x,
        y,
        midpoint_x,
        midpoint_y,
        midpoint_x - dy,
        midpoint_y + dx,
    )
}

fn distance2_line(x: f32, y: f32, ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let dx = bx - ax;
    let dy = by - ay;
    let projection = ((x - ax) * dx + (y - ay) * dy) / (dx * dx + dy * dy);
    line_distance2(x, y, ax + dx * projection, ay + dy * projection)
}

#[cfg(test)]
mod tests {
    use super::super::terraforged_engine::ContinentSettings;
    use super::*;

    fn samples(generator: &AdvancedContinentGenerator) -> [(u32, u32, i32, i32, u64, u32); 5] {
        [
            (0.0, 0.0),
            (12.25, -98.75),
            (-4096.5, 8192.25),
            (123456.75, -0.125),
            (50000.0, 50000.0),
        ]
        .map(|(x, z)| {
            let mut cell = Cell::default();
            generator.apply(&mut cell, x, z);
            (
                cell.continent_id.to_bits(),
                cell.continent_edge.to_bits(),
                cell.continent_x,
                cell.continent_z,
                generator.nearest_center(x, z) as u64,
                generator.edge_value(x, z).to_bits(),
            )
        })
    }

    #[test]
    fn advanced_continents_match_engine_0_3_0() {
        let mut seed = Seed::new(12_345);
        let generator = AdvancedContinentGenerator::new(&mut seed, WorldSettings::default());
        assert_eq!(seed.get(), 12_352);
        assert_eq!(
            samples(&generator),
            [
                (0x3f504337, 0, 1401, -8630, 0x00000579ffffde4a, 0),
                (0x3f504337, 0, 1401, -8630, 0x00000579ffffde4a, 0),
                (0x3f7130b2, 0, 4456, 6530, 0x0000116800001982, 0),
                (
                    0x3b40d400,
                    0x3f4dddfd,
                    126277,
                    2352,
                    0x0001ed4500000930,
                    0x3f4dddfd
                ),
                (
                    0x3e341adc,
                    0x3c87299a,
                    50394,
                    53822,
                    0x0000c4da0000d23e,
                    0x3c87299a
                ),
            ]
        );
        let center = generator.nearest_center(0.0, 0.0);
        assert_eq!(
            generator
                .distance_to_edge(unpack_left(center), unpack_right(center), 0.6, 0.8)
                .to_bits(),
            0x460a6d80
        );
        assert_eq!(
            generator
                .distance_to_ocean(unpack_left(center), unpack_right(center), 0.6, 0.8)
                .to_bits(),
            0x45cc65af
        );
        let rivers = generator.rivermap(unpack_left(center), unpack_right(center));
        let rivers_again = generator.rivermap(unpack_left(center), unpack_right(center));
        assert!(std::sync::Arc::ptr_eq(&rivers, &rivers_again));
        assert_eq!(
            (rivers.x(), rivers.z()),
            (unpack_left(center), unpack_right(center))
        );

        let settings = WorldSettings {
            continent: ContinentSettings {
                continent_scale: 777,
                continent_jitter: 0.85,
                continent_skipping: 0.0,
                continent_size_variance: 0.0,
                ..ContinentSettings::default()
            },
            ..WorldSettings::default()
        };
        let mut seed = Seed::new(i32::MAX - 3);
        let generator = AdvancedContinentGenerator::new(&mut seed, settings);
        assert_eq!(seed.get(), i32::MIN + 3);
        assert_eq!(
            samples(&generator),
            [
                (
                    0x3eff89da,
                    0x3f0b0eec,
                    660,
                    790,
                    0x0000029400000316,
                    0x3f0b0eec
                ),
                (
                    0x3eff89da,
                    0x3e62bf89,
                    660,
                    790,
                    0x0000029400000316,
                    0x3e62bf89
                ),
                (
                    0x3e1e45c0,
                    0x3f800000,
                    -4315,
                    8042,
                    0xffffef2500001f6a,
                    0x3f800000
                ),
                (
                    0x3f5e3263,
                    0x3ece7b6c,
                    124829,
                    1035,
                    0x0001e79d0000040b,
                    0x3ece7b6c
                ),
                (
                    0x3f31e89b,
                    0x3de72252,
                    48297,
                    50160,
                    0x0000bca90000c3f0,
                    0x3de72252
                ),
            ]
        );
    }
}
