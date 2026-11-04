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
        Domain, Interpolation, Line, Module, Source, SourceConfig, SourceKind, cell_vector, floor,
        lerp, map, round, value_coord_2d,
    },
    terraforged_engine::{Cell, ControlPoints, Seed, WorldSettings},
    terraforged_populator::Populator,
    terraforged_util::{pack, unpack_left, unpack_right},
};

/// `AdvancedContinentGenerator.CENTER_CORRECTION`.
const CENTER_CORRECTION: f32 = 0.35;

/// What `apply` writes for a continent that is not skipped.
struct Continent {
    id: f32,
    edge: f32,
    center_x: i32,
    center_z: i32,
}

/// Engine's `AdvancedContinentGenerator` (with its base class `AbstractContinent`), the generator of Engine's
/// default continent type `MULTI_IMPROVED`: warped Voronoi continents whose edge is the distance to the nearest
/// cell bisector, with optional skipping, per-continent size variance and cliff and bay shaping near the coast.
/// Its river map (`getRivermap`, built from `SimpleRiverGenerator`) is not ported yet; building it draws no
/// seeds.
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
}

impl AdvancedContinentGenerator {
    /// Draws seven seeds from `seed`: generator, skipping, variance, the two warp axes, cliff, bay.
    pub fn new(seed: &mut Seed, settings: WorldSettings) -> Self {
        let continent = settings.continent;
        let generator_seed = seed.next();
        let skipping_seed = seed.next();
        let tectonic_scale = continent.continent_scale.wrapping_mul(4);
        let frequency = 1.0 / tectonic_scale as f32;
        let variance_seed = seed.next();
        let warp_scale = round(tectonic_scale as f32 * 0.225);
        let strength = round(tectonic_scale as f32 * 0.33) as f32;
        let octaves = continent.continent_noise_octaves as usize;
        let warp_axis = |seed: i32| {
            source(
                SourceKind::Perlin2,
                seed,
                warp_scale,
                octaves,
                continent.continent_noise_gain,
                continent.continent_noise_lacunarity,
            )
        };
        let warp_x = warp_axis(seed.next());
        let warp_y = warp_axis(seed.next());
        let warp = Domain::domain_warp(warp_x, warp_y, Module::constant(strength));
        let block_scale = || Module::constant(1.0 / frequency);
        let cliff_noise = source(
            SourceKind::Simplex2,
            seed.next(),
            continent.continent_scale / 2,
            2,
            0.5,
            2.0,
        )
        .clamp(Module::constant(0.1), Module::constant(0.25))
        .map(Module::constant(0.0), Module::constant(1.0))
        .frequency(block_scale(), block_scale());
        let bay_noise = source(SourceKind::Simplex, seed.next(), 100, 1, 0.5, 2.0)
            .scale(Module::constant(0.1))
            .bias(Module::constant(0.9))
            .frequency(block_scale(), block_scale());
        Self {
            seed: generator_seed,
            skipping_seed,
            continent_scale: continent.continent_scale,
            jitter: continent.continent_jitter,
            has_skipping: continent.continent_skipping > 0.0,
            skip_threshold: continent.continent_skipping,
            control_points: ControlPoints::new(settings.control_points),
            frequency,
            variance: continent.continent_size_variance,
            variance_seed,
            warp,
            cliff_noise,
            bay_noise,
        }
    }

    /// `getEdgeValue`: `apply` on a fresh cell, then its `continentEdge` (0 for a skipped continent).
    pub fn edge_value(&self, x: f32, z: f32) -> f32 {
        self.continent(x, z).map_or(0.0, |continent| continent.edge)
    }

    /// `getLandValue`: Engine's `Continent` default, the edge value.
    pub fn land_value(&self, x: f32, z: f32) -> f32 {
        self.edge_value(x, z)
    }

    /// `getNearestCenter`: `apply` on a fresh cell, then its corrected centre packed with `PosUtil.pack`
    /// (0, 0 for a skipped continent).
    pub fn nearest_center(&self, x: f32, z: f32) -> i64 {
        self.continent(x, z).map_or(pack(0, 0), |continent| {
            pack(continent.center_x, continent.center_z)
        })
    }

    /// `AbstractContinent.getDistanceToOcean`.
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

    /// `AbstractContinent.getDistanceToEdge`.
    pub fn distance_to_edge(&self, center_x: i32, center_z: i32, dx: f32, dz: f32) -> f32 {
        let mut distance = self.continent_scale.wrapping_mul(4) as f32;
        for _ in 0..10 {
            let x = center_x as f32 + dx * distance;
            let z = center_z as f32 + dz * distance;
            let center = self.nearest_center(x, z);
            // Engine doubles the distance before the check, so the search below starts from twice the probe.
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

    /// The body of Engine's `apply`; `None` where it returns early for a skipped continent.
    fn continent(&self, x: f32, y: f32) -> Option<Continent> {
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
                let (px, py) = self.cell_point(candidate_x, candidate_y);
                let distance = Line::dist2(point_x, point_y, px, py);
                if distance < nearest {
                    cell_point_x = px;
                    cell_point_y = py;
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
                let (px, py) = self.cell_point(candidate_x, candidate_y);
                let distance =
                    bisector_distance2(point_x, point_y, cell_point_x, cell_point_y, px, py);
                sum_x += px;
                sum_y += py;
                if distance < nearest {
                    nearest = distance;
                }
            }
        }
        if self.should_skip(cell_x, cell_y) {
            return None;
        }
        Some(Continent {
            id: cell_value(self.seed, cell_x, cell_y),
            edge: self.distance_value(point_x, point_y, cell_x, cell_y, nearest),
            center_x: self.corrected_center(cell_point_x, sum_x / 8.0),
            center_z: self.corrected_center(cell_point_y, sum_y / 8.0),
        })
    }

    /// `NoiseUtil.cell(seed, x, y)` jittered: the feature point of a lattice cell, in cell units.
    fn cell_point(&self, cell_x: i32, cell_y: i32) -> (f32, f32) {
        let vector = cell_vector(self.seed, cell_x, cell_y);
        (
            cell_x as f32 + vector[0] * self.jitter,
            cell_y as f32 + vector[1] * self.jitter,
        )
    }

    fn should_skip(&self, cell_x: i32, cell_y: i32) -> bool {
        self.has_skipping
            && (cell_x != 0 || cell_y != 0)
            && cell_value(self.skipping_seed, cell_x, cell_y) < self.skip_threshold
    }

    /// `getDistanceValue` (with `getVariedDistanceValue`).
    fn distance_value(&self, x: f32, y: f32, cell_x: i32, cell_y: i32, mut distance: f32) -> f32 {
        if self.variance > 0.0 && (cell_x != 0 || cell_y != 0) {
            let size_value = cell_value(self.variance_seed, cell_x, cell_y);
            distance *= map(size_value, 0.0, self.variance, self.variance);
        }
        distance = f64::from(distance).sqrt() as f32;
        distance = map(distance, 0.05, 0.25, 0.2);
        distance = self.coastal_distance_value(x, y, distance);
        if distance < self.control_points.inland && distance >= self.control_points.shallow_ocean {
            distance = self.coastal_distance_value(x, y, distance);
        }
        distance
    }

    /// `getCoastalDistanceValue`.
    fn coastal_distance_value(&self, x: f32, y: f32, mut distance: f32) -> f32 {
        if distance > self.control_points.shallow_ocean && distance < self.control_points.inland {
            let alpha = distance / self.control_points.inland;
            let cliff = self.cliff_noise.value(x, y);
            distance = lerp(distance * cliff, distance, alpha);
            if distance < self.control_points.shallow_ocean {
                distance = self.control_points.shallow_ocean * self.bay_noise.value(x, y);
            }
        }
        distance
    }

    /// `getCorrectedContinentCentre`: in blocks, truncated like Java's `(int)`.
    fn corrected_center(&self, point: f32, average: f32) -> i32 {
        (lerp(point, average, CENTER_CORRECTION) / self.frequency) as i32
    }
}

impl Populator for AdvancedContinentGenerator {
    /// Writes `continentId`, `continentEdge`, `continentX` and `continentZ`, or nothing for a skipped continent.
    /// Engine does not override `getValue`, so [`Populator::value`] keeps the default: the scratch cell's
    /// `value`, which `apply` never writes (always 0).
    fn apply(&self, cell: &mut Cell, x: f32, y: f32) {
        if let Some(continent) = self.continent(x, y) {
            cell.continent_id = continent.id;
            cell.continent_edge = continent.edge;
            cell.continent_x = continent.center_x;
            cell.continent_z = continent.center_z;
        }
    }
}

/// `Source.build(seed, scale, octaves).gain(gain).lacunarity(lacunarity)` built as `kind`.
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

/// `AbstractContinent.getCellValue`.
fn cell_value(seed: i32, cell_x: i32, cell_y: i32) -> f32 {
    0.5 + value_coord_2d(seed, cell_x, cell_y) * 0.5
}

/// `getDistance`: the squared distance from (x, y) to the bisector of a and b.
fn bisector_distance2(x: f32, y: f32, ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let mx = (ax + bx) * 0.5;
    let my = (ay + by) * 0.5;
    let dx = bx - ax;
    let dy = by - ay;
    let nx = -dy;
    let ny = dx;
    distance2_line(x, y, mx, my, mx + nx, my + ny)
}

/// `getDistance2Line`: the squared distance from (x, y) to the infinite line through a and b.
fn distance2_line(x: f32, y: f32, ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let dx = bx - ax;
    let dy = by - ay;
    let v = ((x - ax) * dx + (y - ay) * dy) / (dx * dx + dy * dy);
    Line::dist2(x, y, ax + dx * v, ay + dy * v)
}

#[cfg(test)]
mod tests {
    use super::super::terraforged_continent::tests::{
        DIRECTIONS, XS, filled, mix, mix_cell, mix_f,
    };
    use super::super::terraforged_engine::{
        ContinentSettings, TerrainRegistry, WorldControlPoints,
    };
    use super::*;

    /// The configs of the Java oracle (`.local/terraforged/oracles/continent/AdvancedContinentOracle.java`):
    /// (seed, settings).
    fn config(index: usize) -> (i32, WorldSettings) {
        let mut settings = WorldSettings {
            seed: 246_813_579,
            ..WorldSettings::default()
        };
        let continent = &mut settings.continent;
        let seed = match index {
            1 => {
                continent.continent_scale = 777;
                continent.continent_jitter = 0.85;
                continent.continent_skipping = 0.0;
                continent.continent_size_variance = 0.0;
                i32::MAX - 3
            }
            2 => {
                continent.continent_scale = 1;
                continent.continent_jitter = 1.0;
                continent.continent_skipping = 1.0;
                continent.continent_size_variance = 1.0;
                settings.control_points = WorldControlPoints {
                    inland: 0.9,
                    shallow_ocean: 0.05,
                    ..WorldControlPoints::default()
                };
                -98_765
            }
            3 => {
                continent.continent_scale = i32::MAX;
                continent.continent_jitter = f32::NAN;
                continent.continent_noise_octaves = 1;
                i32::MIN + 1
            }
            4 => {
                continent.continent_scale = 2;
                continent.continent_jitter = 0.0;
                continent.continent_skipping = 0.75;
                continent.continent_size_variance = 0.5;
                continent.continent_noise_octaves = 0;
                continent.continent_noise_gain = -0.5;
                continent.continent_noise_lacunarity = 0.5;
                7
            }
            5 => {
                continent.continent_scale = 0;
                continent.continent_skipping = f32::NAN;
                continent.continent_size_variance = f32::NAN;
                0
            }
            _ => 12_345,
        };
        (seed, settings)
    }

    /// Folds what the oracle prints for one config into `h`; returns the seed cursor after construction.
    fn fold_config(h: &mut i64, index: usize) -> i32 {
        let registry = TerrainRegistry::new();
        let (seed_value, settings) = config(index);
        let mut seed = Seed::new(seed_value);
        let generator = AdvancedContinentGenerator::new(&mut seed, settings);
        mix(h, i64::from(seed.get()));
        for x in XS {
            for z in XS {
                let mut cell = Cell::new(&registry);
                generator.apply(&mut cell, x, z);
                mix_cell(h, &cell);
                let mut cell = filled(&registry);
                generator.apply(&mut cell, x, z);
                mix_cell(h, &cell);
                mix_f(h, generator.value(&registry, x, z));
                mix_f(h, generator.edge_value(x, z));
                mix_f(h, generator.land_value(x, z));
                mix(h, generator.nearest_center(x, z));
            }
        }
        let first = generator.nearest_center(0.0, 0.0);
        let second = generator.nearest_center(-4096.5, 8192.25);
        let centers = [
            (unpack_left(first), unpack_right(first)),
            (unpack_left(second), unpack_right(second)),
            (0, 0),
            (i32::MAX, i32::MIN),
        ];
        for (center_x, center_z) in centers {
            for (dx, dz) in DIRECTIONS {
                mix_f(h, generator.distance_to_edge(center_x, center_z, dx, dz));
                mix_f(h, generator.distance_to_ocean(center_x, center_z, dx, dz));
            }
        }
        seed.get()
    }

    #[test]
    fn advanced_continents_match_engine_0_3_0_over_a_grid() {
        // Per config: the seed cursor after construction and the running hash after it (`config N hash=`).
        const EXPECTED: [(i32, i64); 6] = [
            (12_352, 5_160_731_105_250_637_841),
            (-2_147_483_645, 1_708_800_664_643_352_733),
            (-98_758, 8_484_870_971_288_875_850),
            (-2_147_483_640, -4_291_447_758_620_258_839),
            (14, -370_435_378_152_573_729),
            (7, -3_116_283_162_433_994_216),
        ];
        let mut h = 17;
        for (index, (seed, hash)) in EXPECTED.into_iter().enumerate() {
            assert_eq!(fold_config(&mut h, index), seed, "config {index} seed");
            assert_eq!(h, hash, "config {index} hash");
        }
    }

    fn samples(generator: &AdvancedContinentGenerator) -> [(u32, u32, i32, i32, u64, u32); 5] {
        [
            (0.0, 0.0),
            (12.25, -98.75),
            (-4096.5, 8192.25),
            (123456.75, -0.125),
            (50000.0, 50000.0),
        ]
        .map(|(x, z)| {
            let mut cell = Cell::new(&TerrainRegistry::new());
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
        // Engine does not override `getValue`: it reads the scratch cell's `value`, which `apply` never writes.
        assert_eq!(
            generator
                .value(&TerrainRegistry::new(), 123_456.75, -0.125)
                .to_bits(),
            0
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
