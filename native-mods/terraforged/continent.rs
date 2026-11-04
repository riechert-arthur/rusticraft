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
        DistanceFunction, Domain, EdgeFunction, Interpolation, Module, Source, SourceConfig,
        SourceKind, cell_vector, floor, map, value_coord_2d,
    },
    terraforged_engine::{
        Cell, ContinentShape, ControlPoints, Seed, TerrainRegistry, WorldSettings,
    },
    terraforged_populator::Populator,
    terraforged_util::{pack, unpack_left, unpack_right},
};

/// The nearest jittered cell of a [`SimpleContinentGenerator::search`], in cell units.
struct Search {
    cell_x: i32,
    cell_y: i32,
    center_x: f32,
    center_y: f32,
    distance: f32,
    distance2: f32,
}

/// Engine's simple continent generator (`ContinentGenerator`, `MultiContinentGenerator`,
/// `SingleContinentGenerator`): continents are jittered cells of a warped cellular lattice, shaped by a simplex
/// mask near their edges. Its river map (`getRivermap`, built from `SimpleRiverGenerator`) is not ported yet;
/// building it draws no seeds.
pub struct SimpleContinentGenerator {
    seed: i32,
    frequency: f32,
    continent_scale: i32,
    distance_function: DistanceFunction,
    control_points: ControlPoints,
    clamp_min: f32,
    clamp_max: f32,
    clamp_range: f32,
    offset_alpha: f32,
    warp: Domain,
    shape: Module,
    single_center: Option<(i32, i32)>,
}

impl SimpleContinentGenerator {
    /// `MultiContinentGenerator`: draws four seeds (generator, two warps, shape) from `seed`.
    pub fn multi(seed: &mut Seed, settings: WorldSettings) -> Self {
        Self::new(seed, settings, false)
    }

    /// `SingleContinentGenerator`: as [`Self::multi`], keeping only the continent nearest to (0, 0).
    pub fn single(seed: &mut Seed, settings: WorldSettings) -> Self {
        Self::new(seed, settings, true)
    }

    fn new(seed: &mut Seed, settings: WorldSettings, single: bool) -> Self {
        let tectonic_scale = settings.continent.continent_scale.wrapping_mul(4);
        let continent_scale = settings.continent.continent_scale / 2;
        let generator_seed = seed.next();
        let distance_function = match settings.continent.continent_shape {
            ContinentShape::Euclidean => DistanceFunction::Euclidean,
            ContinentShape::Manhattan => DistanceFunction::Manhattan,
            ContinentShape::Natural => DistanceFunction::Natural,
        };
        let first_warp = noise_warp(SourceKind::Perlin, seed.next(), 20, 2, 20.0);
        let second_warp = noise_warp(
            SourceKind::Simplex,
            seed.next(),
            continent_scale,
            3,
            continent_scale as f32,
        );
        let shape = source(
            SourceKind::Simplex,
            seed.next(),
            settings.continent.continent_scale.wrapping_mul(2),
            1,
        )
        .bias(Module::constant(0.65))
        .clamp(Module::constant(0.0), Module::constant(1.0));
        let mut generator = Self {
            seed: generator_seed,
            frequency: 1.0 / tectonic_scale as f32,
            continent_scale,
            distance_function,
            control_points: ControlPoints::new(settings.control_points),
            clamp_min: 0.2,
            clamp_max: 1.0,
            clamp_range: 0.8,
            offset_alpha: settings.continent.continent_jitter,
            warp: first_warp.compound(second_warp),
            shape,
            single_center: None,
        };
        if single {
            let center = generator.nearest_center(0.0, 0.0);
            generator.single_center = Some((unpack_left(center), unpack_right(center)));
        }
        generator
    }

    /// `getEdgeValue`: the shaped continent edge at a block position. Unlike [`Populator::value`], a
    /// single-continent generator does not zero it outside its continent.
    pub fn edge_value(&self, x: f32, y: f32) -> f32 {
        let search = self.search(x, y);
        let edge = self.cell_edge_value(search.distance, search.distance2);
        edge * self.shape_value(x, y, edge)
    }

    /// `getLandValue`: Engine's `Continent` default, the edge value.
    pub fn land_value(&self, x: f32, z: f32) -> f32 {
        self.edge_value(x, z)
    }

    /// `getNearestCenter`: the nearest continent centre in blocks, packed with `PosUtil.pack`.
    pub fn nearest_center(&self, x: f32, z: f32) -> i64 {
        let search = self.search(x, z);
        pack(
            (search.center_x / self.frequency) as i32,
            (search.center_y / self.frequency) as i32,
        )
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
            // Engine doubles the distance before the check, so the search below starts from twice the probe.
            distance += distance;
            if unpack_left(center) == center_x && unpack_right(center) == center_z {
                continue;
            }
            let mut low = 0.0;
            let mut high = distance;
            for _ in 0..50 {
                let mid = (low + high) / 2.0;
                let point_x = center_x as f32 + dx * mid;
                let point_z = center_z as f32 + dz * mid;
                let center = self.nearest_center(point_x, point_z);
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

    /// The 3x3 cellular search Engine's `apply`, `getEdgeValue` and `getNearestCenter` each inline: warp,
    /// scale by the frequency, then find the nearest and second-nearest jittered cell centres. The three
    /// copies compare the same distances in the same order, so one search serves all of them.
    fn search(&self, x: f32, y: f32) -> Search {
        let point_x = (x + self.warp.offset_x(x, y)) * self.frequency;
        let point_y = (y + self.warp.offset_y(x, y)) * self.frequency;
        let x_index = floor(point_x);
        let y_index = floor(point_y);
        let mut search = Search {
            cell_x: x_index,
            cell_y: y_index,
            center_x: point_x,
            center_y: point_y,
            distance: 999_999.0,
            distance2: 999_999.0,
        };
        for delta_y in -1_i32..=1 {
            for delta_x in -1_i32..=1 {
                let cell_x = x_index.wrapping_add(delta_x);
                let cell_y = y_index.wrapping_add(delta_y);
                let vector = cell_vector(self.seed, cell_x, cell_y);
                let center_x = cell_x as f32 + vector[0] * self.offset_alpha;
                let center_y = cell_y as f32 + vector[1] * self.offset_alpha;
                let distance = self
                    .distance_function
                    .apply(center_x - point_x, center_y - point_y);
                if distance < search.distance {
                    search.distance2 = search.distance;
                    search.distance = distance;
                    search.center_x = center_x;
                    search.center_y = center_y;
                    search.cell_x = cell_x;
                    search.cell_y = cell_y;
                } else if distance < search.distance2 {
                    search.distance2 = distance;
                }
            }
        }
        search
    }

    fn cell_identity(&self, cell_x: i32, cell_y: i32) -> f32 {
        map(value_coord_2d(self.seed, cell_x, cell_y), -1.0, 1.0, 2.0)
    }

    fn cell_edge_value(&self, distance: f32, distance2: f32) -> f32 {
        let edge = EdgeFunction::Distance2Div;
        let (min, max, range) = edge.bounds();
        let value = 1.0 - map(edge.apply(distance, distance2), min, max, range);
        if value <= self.clamp_min {
            0.0
        } else if value >= self.clamp_max {
            1.0
        } else {
            (value - self.clamp_min) / self.clamp_range
        }
    }

    fn shape_value(&self, x: f32, z: f32, edge: f32) -> f32 {
        if edge >= self.control_points.inland {
            1.0
        } else {
            self.shape.value(x, z) * (edge / self.control_points.inland)
        }
    }
}

impl Populator for SimpleContinentGenerator {
    /// Writes `continentId`, `continentEdge`, `continentX` and `continentZ`; leaves every other field.
    fn apply(&self, cell: &mut Cell, x: f32, y: f32) {
        let search = self.search(x, y);
        cell.continent_id = self.cell_identity(search.cell_x, search.cell_y);
        cell.continent_edge = self.cell_edge_value(search.distance, search.distance2);
        cell.continent_x = (search.center_x / self.frequency) as i32;
        cell.continent_z = (search.center_y / self.frequency) as i32;
        cell.continent_edge *= self.shape_value(x, y, cell.continent_edge);
        if let Some(center) = self.single_center
            && (cell.continent_x, cell.continent_z) != center
        {
            cell.continent_id = 0.0;
            cell.continent_edge = 0.0;
            cell.continent_x = 0;
            cell.continent_z = 0;
        }
    }

    /// `getValue`: `apply` on a scratch cell, then its `continentEdge` (so a single-continent generator
    /// returns 0 outside its continent, unlike [`Self::edge_value`]).
    fn value(&self, registry: &TerrainRegistry, x: f32, z: f32) -> f32 {
        let mut cell = Cell::new(registry);
        self.apply(&mut cell, x, z);
        cell.continent_edge
    }
}

fn source(kind: SourceKind, seed: i32, scale: i32, octaves: usize) -> Module {
    Module::source(Source::new(
        kind,
        SourceConfig {
            seed,
            octaves,
            gain: 0.5,
            frequency: 1.0 / scale as f32,
            lacunarity: 2.0,
            interpolation: Interpolation::Curve3,
        },
    ))
}

fn noise_warp(kind: SourceKind, seed: i32, scale: i32, octaves: usize, strength: f32) -> Domain {
    Domain::domain_warp(
        source(kind, seed, scale, octaves),
        source(kind, seed.wrapping_add(1), scale, octaves),
        Module::constant(strength),
    )
}

#[cfg(test)]
pub(super) mod tests {
    use super::super::terraforged_engine::{BiomeType, ContinentSettings, WorldControlPoints};
    use super::*;

    /// The fold the Java oracle (`.local/terraforged/oracles/continent/`) uses: `h = h * 1_000_003 ^ v` with
    /// Java `long` wrap, `v` widened from `int` with sign extension.
    pub(crate) fn mix(h: &mut i64, v: i64) {
        *h = h.wrapping_mul(1_000_003) ^ v;
    }

    /// Raw float bits, every NaN as Java's canonical `0x7fc00000` (the oracle's `fb`).
    pub(crate) fn mix_f(h: &mut i64, v: f32) {
        let bits = if v.is_nan() { 0x7fc0_0000 } else { v.to_bits() };
        mix(h, i64::from(bits as i32));
    }

    pub(crate) fn mix_cell(h: &mut i64, cell: &Cell) {
        for value in [
            cell.value,
            cell.erosion,
            cell.sediment,
            cell.gradient,
            cell.moisture,
            cell.temperature,
        ] {
            mix_f(h, value);
        }
        mix_f(h, cell.continent_id);
        mix_f(h, cell.continent_edge);
        mix_f(h, cell.terrain_region_id);
        mix_f(h, cell.terrain_region_edge);
        mix(h, cell.terrain_region_center);
        for value in [
            cell.biome_region_id,
            cell.biome_region_edge,
            cell.macro_biome_id,
            cell.river_mask,
        ] {
            mix_f(h, value);
        }
        mix(h, i64::from(cell.continent_x));
        mix(h, i64::from(cell.continent_z));
        mix(h, i64::from(cell.erosion_mask));
        mix(h, i64::from(cell.terrain.id()));
        mix(h, i64::from(cell.biome.id()));
        mix(h, i64::from(cell.is_absent()));
    }

    /// Every field set, so the fold shows which fields `apply` leaves untouched (the oracle's `filled`).
    pub(crate) fn filled(registry: &TerrainRegistry) -> Cell {
        let mut cell = Cell::new(registry);
        cell.value = 0.75;
        cell.erosion = 0.25;
        cell.sediment = 0.125;
        cell.gradient = 0.875;
        cell.moisture = 0.2;
        cell.temperature = 0.8;
        cell.continent_id = 0.1;
        cell.continent_edge = 0.9;
        cell.terrain_region_id = 0.3;
        cell.terrain_region_edge = 0.7;
        cell.terrain_region_center = 123_456_789;
        cell.biome_region_id = 0.4;
        cell.biome_region_edge = 0.6;
        cell.macro_biome_id = 0.55;
        cell.river_mask = 0.45;
        cell.continent_x = -123;
        cell.continent_z = 456;
        cell.erosion_mask = true;
        cell.terrain = registry.get_by_id(14);
        cell.biome = BiomeType::Alpine;
        cell
    }

    // Zeros, grid points, huge values, ±inf, NaN, the `(int)` saturation edges and the smallest subnormal.
    #[rustfmt::skip]
    pub(crate) const XS: [f32; 19] = [
        0.0, -0.0, 12.25, -98.75, -4096.5, 8192.25, 123_456.75, -1500.5, 2400.75, 1e9, -1e9, 3e38, -f32::MAX,
        f32::INFINITY, f32::NEG_INFINITY, f32::NAN, 2_147_483_648.0, -2_147_483_904.0, 1e-45,
    ];
    #[rustfmt::skip]
    pub(crate) const DIRECTIONS: [(f32, f32); 7] = [
        (0.6, 0.8), (-1.0, 0.0), (0.0, -1.0), (0.707_106_77, -0.707_106_77), (0.0, 0.0), (f32::NAN, 0.0),
        (1e30, 1e30),
    ];

    /// The oracle's six configs: (single, seed, settings).
    fn config(index: usize) -> (bool, i32, WorldSettings) {
        let mut settings = WorldSettings {
            seed: 246_813_579,
            ..WorldSettings::default()
        };
        let continent = &mut settings.continent;
        let seed = match index {
            2 => {
                continent.continent_shape = ContinentShape::Manhattan;
                continent.continent_jitter = 1.0;
                continent.continent_scale = 777;
                settings.control_points = WorldControlPoints {
                    inland: 0.9,
                    shallow_ocean: 0.05,
                    ..WorldControlPoints::default()
                };
                -98_765
            }
            3 => {
                continent.continent_shape = ContinentShape::Natural;
                continent.continent_jitter = 0.0;
                continent.continent_scale = 1;
                i32::MAX
            }
            4 => {
                continent.continent_shape = ContinentShape::Natural;
                continent.continent_jitter = 0.35;
                continent.continent_scale = i32::MAX;
                i32::MIN + 1
            }
            5 => {
                continent.continent_scale = 5001;
                continent.continent_jitter = f32::NAN;
                7
            }
            _ => 12_345,
        };
        debug_assert_eq!(ContinentSettings::DEFAULT_SCALE, 3_000);
        (index % 2 == 1, seed, settings)
    }

    /// Folds what the oracle prints for one config into `h`; returns the seed cursor after construction.
    fn fold_config(h: &mut i64, index: usize) -> i32 {
        let registry = TerrainRegistry::new();
        let (single, seed_value, settings) = config(index);
        let mut seed = Seed::new(seed_value);
        let generator = if single {
            SimpleContinentGenerator::single(&mut seed, settings)
        } else {
            SimpleContinentGenerator::multi(&mut seed, settings)
        };
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
    fn simple_continents_match_engine_0_3_0_over_a_grid() {
        // Per config: the seed cursor after construction and the running hash after it (`config N hash=`).
        const EXPECTED: [(i32, i64); 6] = [
            (12_349, -2_484_171_936_580_001_758),
            (12_349, -5_638_837_142_254_385_281),
            (-98_761, -7_497_355_774_646_880_631),
            (-2_147_483_645, 6_267_758_184_447_195_761),
            (-2_147_483_643, -6_314_250_473_245_537_182),
            (11, 8_344_191_055_440_514_107),
        ];
        let mut h = 17;
        for (index, (seed, hash)) in EXPECTED.into_iter().enumerate() {
            assert_eq!(fold_config(&mut h, index), seed, "config {index} seed");
            assert_eq!(h, hash, "config {index} hash");
        }
    }

    #[test]
    fn simple_continent_cells_match_engine_0_3_0() {
        let registry = TerrainRegistry::new();
        let settings = config(0).2;
        let mut seed = Seed::new(12_345);
        let multi = SimpleContinentGenerator::multi(&mut seed, settings);
        let mut seed = Seed::new(12_345);
        let single = SimpleContinentGenerator::single(&mut seed, settings);
        // (x, z, continentId, continentEdge, continentX, continentZ) after `apply` on a fresh cell; the single
        // generator zeroes all four outside the continent around (0, 0).
        for (generator, expected) in [
            (
                &multi,
                [
                    (0x3f50_4337, 0, 538, -8737),
                    (0x3f50_4337, 0x3c5b_6cf7, 538, -8737),
                    (0x3f16_fc4e, 0x3cc7_4025, -11502, 4962),
                ],
            ),
            (
                &single,
                [
                    (0x3f50_4337, 0, 538, -8737),
                    (0x3f50_4337, 0x3c5b_6cf7, 538, -8737),
                    (0, 0, 0, 0),
                ],
            ),
        ] {
            for ((x, z), (id, edge, cx, cz)) in [(0.0, 0.0), (12.25, -98.75), (-4096.5, 8192.25)]
                .into_iter()
                .zip(expected)
            {
                let mut cell = Cell::new(&registry);
                generator.apply(&mut cell, x, z);
                assert_eq!(
                    (
                        cell.continent_id.to_bits(),
                        cell.continent_edge.to_bits(),
                        cell.continent_x,
                        cell.continent_z
                    ),
                    (id, edge, cx, cz)
                );
                assert_eq!(generator.value(&registry, x, z).to_bits(), edge);
            }
        }
        // `getEdgeValue` ignores the single-continent filter; `getValue` does not.
        assert_eq!(single.edge_value(-4096.5, 8192.25).to_bits(), 0x3cc7_4025);
        assert_eq!(
            single.nearest_center(-4096.5, 8192.25) as u64,
            0xffff_d312_0000_1362
        );
    }
}
