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
        DistanceFunction, Domain, EdgeFunction, Module, cell_vector, floor, java_pow,
        map as noise_map, value_coord_2d,
    },
    terraforged_engine::Cell,
    terraforged_populator::Populator,
};
use std::sync::Arc;

/// Engine's `RegionConfig`: the terrain-region seed, the region size in blocks (`scale`) and the
/// warp axes and strength. Engine's `Heightmap` builds it from `seed.offset(789124).get()`,
/// `terrainRegionSize` (1200 by default) and two 1-octave simplex axes at scale 400 seeded from
/// `seed.offset(8934)` (`next()` twice), with a strength of 200.
#[derive(Clone)]
pub struct RegionConfig {
    pub seed: i32,
    pub scale: i32,
    pub warp_x: Module,
    pub warp_z: Module,
    pub warp_strength: f64,
}

impl RegionConfig {
    pub fn new(seed: i32, scale: i32, warp_x: Module, warp_z: Module, warp_strength: f64) -> Self {
        Self {
            seed,
            scale,
            warp_x,
            warp_z,
            warp_strength,
        }
    }
}

/// Engine's `RegionModule` populator: it warps the point, finds the nearest jittered (0.7) cell
/// at `1 / scale` with the natural distance, and writes only `terrain_region_id` (the nearest
/// cell's `valCoord2D` mapped to 0..1), `terrain_region_edge` (`DISTANCE_2_DIV` mapped, inverted,
/// raised to 1.5 and stretched from 0..0.5 to 0..1) and `terrain_region_center` (the nearest feature
/// point in blocks, `PosUtil.pack`ed after Java's saturating `(int)` casts). Its seed is the
/// config's plus 7, wrapping like Java's `int`.
pub struct RegionModule {
    seed: i32,
    frequency: f32,
    edge_min: f32,
    edge_max: f32,
    edge_range: f32,
    warp: Domain,
}

impl RegionModule {
    pub fn new(config: RegionConfig) -> Self {
        let edge_min = 0.0;
        let edge_max = 0.5;
        Self {
            seed: config.seed.wrapping_add(7),
            frequency: 1.0 / config.scale as f32,
            edge_min,
            edge_max,
            edge_range: edge_max - edge_min,
            warp: Domain::domain_warp(
                config.warp_x,
                config.warp_z,
                Module::constant(config.warp_strength as f32),
            ),
        }
    }

    fn cell_value(&self, cell_x: i32, cell_y: i32) -> f32 {
        noise_map(value_coord_2d(self.seed, cell_x, cell_y), -1.0, 1.0, 2.0)
    }

    fn edge_value(&self, distance: f32, distance2: f32) -> f32 {
        let edge = EdgeFunction::Distance2Div;
        let value = edge.apply(distance, distance2);
        let (min, max, range) = edge.bounds();
        let mut edge_value = 1.0 - noise_map(value, min, max, range);
        edge_value = java_pow(edge_value, 1.5);
        if edge_value < self.edge_min {
            return 0.0;
        }
        if edge_value > self.edge_max {
            return 1.0;
        }
        (edge_value - self.edge_min) / self.edge_range
    }
}

impl Populator for RegionModule {
    fn apply(&self, cell: &mut Cell, x: f32, y: f32) {
        let mut point_x = x + self.warp.offset_x(x, y);
        let mut point_y = y + self.warp.offset_y(x, y);
        point_x *= self.frequency;
        point_y *= self.frequency;
        let mut cell_x = 0;
        let mut cell_y = 0;
        let mut center_x = 0.0;
        let mut center_y = 0.0;
        let x_index = floor(point_x);
        let y_index = floor(point_y);
        let mut edge_distance = f32::MAX;
        let mut edge_distance2 = f32::MAX;
        for delta_y in -1_i32..=1 {
            for delta_x in -1_i32..=1 {
                let candidate_x = x_index.wrapping_add(delta_x);
                let candidate_y = y_index.wrapping_add(delta_y);
                let vector = cell_vector(self.seed, candidate_x, candidate_y);
                let vector_x = candidate_x as f32 + vector[0] * 0.7;
                let vector_y = candidate_y as f32 + vector[1] * 0.7;
                let distance =
                    DistanceFunction::Natural.apply(vector_x - point_x, vector_y - point_y);
                if distance < edge_distance {
                    edge_distance2 = edge_distance;
                    edge_distance = distance;
                    center_x = vector_x;
                    center_y = vector_y;
                    cell_x = candidate_x;
                    cell_y = candidate_y;
                } else if distance < edge_distance2 {
                    edge_distance2 = distance;
                }
            }
        }
        cell.terrain_region_id = self.cell_value(cell_x, cell_y);
        cell.terrain_region_edge = self.edge_value(edge_distance, edge_distance2);
        cell.terrain_region_center = pack(center_x / self.frequency, center_y / self.frequency);
    }
}

pub struct RegionSelector {
    max_index: i32,
    nodes: Vec<Arc<dyn Populator>>,
}

impl RegionSelector {
    pub fn new(populators: Vec<Arc<dyn Populator>>) -> Self {
        let nodes = Self::weighted_array(&populators);
        Self {
            max_index: nodes.len() as i32 - 1,
            nodes,
        }
    }

    pub fn get(&self, identity: f32) -> &dyn Populator {
        let index = noise_round(identity * self.max_index as f32);
        self.nodes[index as usize].as_ref()
    }

    fn weighted_array(populators: &[Arc<dyn Populator>]) -> Vec<Arc<dyn Populator>> {
        let mut smallest = f32::MAX;
        for populator in populators {
            if let Some(weight) = populator.terrain_weight() {
                if weight == 0.0 {
                    continue;
                }
                smallest = smallest.min(weight);
            } else {
                smallest = smallest.min(1.0);
            }
        }
        if smallest == f32::MAX {
            return populators.to_vec();
        }
        let mut result = Vec::new();
        for populator in populators {
            let count = match populator.terrain_weight() {
                Some(0.0) => continue,
                Some(weight) => java_round(weight / smallest),
                None => java_round(1.0 / smallest),
            };
            for _ in 0..count.max(0) {
                result.push(Arc::clone(populator));
            }
        }
        if result.is_empty() {
            populators.to_vec()
        } else {
            result
        }
    }
}

impl Populator for RegionSelector {
    fn apply(&self, cell: &mut Cell, x: f32, y: f32) {
        self.get(cell.terrain_region_id).apply(cell, x, y);
    }
}

pub struct RegionLerper {
    lower: Arc<dyn Populator>,
    upper: Arc<dyn Populator>,
}

impl RegionLerper {
    pub fn new(lower: Arc<dyn Populator>, upper: Arc<dyn Populator>) -> Self {
        Self { lower, upper }
    }
}

impl Populator for RegionLerper {
    fn apply(&self, cell: &mut Cell, x: f32, y: f32) {
        let alpha = cell.terrain_region_edge;
        if alpha == 0.0 {
            self.lower.apply(cell, x, y);
            return;
        }
        if alpha == 1.0 {
            self.upper.apply(cell, x, y);
            return;
        }
        self.lower.apply(cell, x, y);
        let lower_value = cell.value;
        self.upper.apply(cell, x, y);
        let upper_value = cell.value;
        cell.value = lower_value + alpha * (upper_value - lower_value);
    }
}

/// `PosUtil.pack(float, float)`: both halves truncated like Java's `(int)` (saturating, NaN to 0).
fn pack(left: f32, right: f32) -> i64 {
    let left = left as i32;
    let right = right as i32;
    (i64::from(left) << 32) | (i64::from(right) & 0xffff_ffff)
}

fn java_round(value: f32) -> i32 {
    (value + 0.5).floor() as i32
}

fn noise_round(value: f32) -> i32 {
    if value >= 0.0 {
        (value + 0.5) as i32
    } else {
        (value - 0.5) as i32
    }
}

#[cfg(test)]
mod tests {
    use super::super::{
        terraforged::{Interpolation, Source, SourceConfig, SourceKind},
        terraforged_engine::{BiomeType, Seed, TerrainRegistry},
        terraforged_populator::TerrainPopulator,
    };
    use super::*;
    use std::fmt::Write as _;

    // Java's `BiomeType` names in id order, for the oracle's text.
    const BIOME_NAMES: [&str; 11] = [
        "TROPICAL_RAINFOREST",
        "SAVANNA",
        "DESERT",
        "TEMPERATE_RAINFOREST",
        "TEMPERATE_FOREST",
        "GRASSLAND",
        "COLD_STEPPE",
        "STEPPE",
        "TAIGA",
        "TUNDRA",
        "ALPINE",
    ];

    fn bits(value: f32) -> String {
        format!("{:08x}", value.to_bits())
    }

    fn dump(cell: &Cell) -> String {
        format!(
            "value={} erosion={} sediment={} gradient={} moisture={} temperature={} continentId={} continentEdge={} trId={} trEdge={} trCenter={} brId={} brEdge={} macro={} river={} cx={} cz={} emask={} terrain={} biome={} absent={}",
            bits(cell.value),
            bits(cell.erosion),
            bits(cell.sediment),
            bits(cell.gradient),
            bits(cell.moisture),
            bits(cell.temperature),
            bits(cell.continent_id),
            bits(cell.continent_edge),
            bits(cell.terrain_region_id),
            bits(cell.terrain_region_edge),
            cell.terrain_region_center,
            bits(cell.biome_region_id),
            bits(cell.biome_region_edge),
            bits(cell.macro_biome_id),
            bits(cell.river_mask),
            cell.continent_x,
            cell.continent_z,
            cell.erosion_mask,
            cell.terrain.id(),
            BIOME_NAMES[cell.biome.id() as usize],
            cell.is_absent(),
        )
    }

    fn filled(registry: &TerrainRegistry) -> Cell {
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

    /// `Source.build(seed, scale, octaves)` built as `kind` (Engine's builder defaults otherwise).
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

    fn perlin(seed: i32, scale: i32, octaves: usize) -> Module {
        source(SourceKind::Perlin, seed, scale, octaves)
    }

    fn fnv1a(text: &str) -> u64 {
        text.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
        })
    }

    fn run(out: &mut String, registry: &TerrainRegistry, name: &str, config: RegionConfig) {
        let module = RegionModule::new(config);
        let xs = [
            0.0,
            12.25,
            -98.75,
            -4096.5,
            8192.25,
            123_456.75,
            -3.0e7,
            1.0e9,
            -2.5e9,
            3.4e38,
            f32::NAN,
            f32::INFINITY,
            f32::NEG_INFINITY,
        ];
        for x in xs {
            for z in xs {
                let mut fresh = Cell::new(registry);
                module.apply(&mut fresh, x, z);
                let mut full = filled(registry);
                module.apply(&mut full, x, z);
                let prefix = format!("{name} {} {}", bits(x), bits(z));
                writeln!(
                    out,
                    "{prefix} getValue={}",
                    bits(module.value(registry, x, z))
                )
                .unwrap();
                writeln!(out, "{prefix} fresh {}", dump(&fresh)).unwrap();
                writeln!(out, "{prefix} filled {}", dump(&full)).unwrap();
            }
        }
    }

    /// Rebuilds the text `RegionOracle` prints from Engine 0.3.0 (TerraForged's
    /// `.local/terraforged/oracles/region/`): every cell field after `apply` on a fresh and on a
    /// fully set cell over a 13 x 13 grid (negative, large, NaN and infinite coordinates), for
    /// Heightmap's own config and for Perlin warps, an int-overflowing seed, zero and denormal
    /// strengths and zero, negative, unit and `i32::MAX` scales.
    fn oracle_text() -> String {
        let registry = TerrainRegistry::new();
        let mut out = String::new();
        let world = Seed::new(12345);
        let region_seed = world.offset(789_124);
        let mut region_warp = world.offset(8934);
        let warp_x = source(SourceKind::Simplex, region_warp.next(), 400, 1);
        let warp_z = source(SourceKind::Simplex, region_warp.next(), 400, 1);
        let heightmap = RegionConfig::new(region_seed.get(), 1200, warp_x, warp_z, 200.0);
        run(&mut out, &registry, "heightmap", heightmap);
        let cases: [(&str, i32, i32, Module, Module, f64); 7] = [
            (
                "perlin",
                123,
                400,
                perlin(7, 100, 2),
                perlin(8, 100, 2),
                50.5,
            ),
            (
                "overflow",
                i32::MAX - 3,
                37,
                perlin(-99, 17, 3),
                perlin(-98, 17, 3),
                -12.25,
            ),
            ("still", -7, 300, perlin(1, 64, 1), perlin(2, 64, 1), 0.0),
            (
                "unit",
                i32::MIN,
                1,
                perlin(3, 50, 1),
                perlin(4, 50, 1),
                1.0e-50,
            ),
            ("zero", 42, 0, perlin(5, 50, 1), perlin(6, 50, 1), 10.0),
            (
                "negative",
                42,
                -500,
                perlin(5, 50, 1),
                perlin(6, 50, 1),
                1.0e40,
            ),
            (
                "huge",
                0,
                i32::MAX,
                perlin(9, 50, 1),
                perlin(10, 50, 1),
                3.0e38,
            ),
        ];
        for (name, seed, scale, warp_x, warp_z, strength) in cases {
            let config = RegionConfig::new(seed, scale, warp_x, warp_z, strength);
            run(&mut out, &registry, name, config);
        }
        out
    }

    #[test]
    fn region_module_matches_engine_0_3_0() {
        let text = oracle_text();
        let mut lines = text.lines();
        assert_eq!(
            lines.next(),
            Some("heightmap 00000000 00000000 getValue=00000000")
        );
        assert_eq!(
            lines.next(),
            Some(
                "heightmap 00000000 00000000 fresh value=00000000 erosion=00000000 sediment=00000000 gradient=00000000 moisture=3f000000 temperature=3f000000 continentId=00000000 continentEdge=00000000 trId=3f4f60f4 trEdge=3e7c4fc8 trCenter=725849472526 brId=00000000 brEdge=3f800000 macro=00000000 river=3f800000 cx=0 cz=0 emask=false terrain=0 biome=GRASSLAND absent=false"
            )
        );
        assert_eq!(text.lines().count(), 4056);
        assert_eq!(text.len(), 1_004_508);
        assert_eq!(fnv1a(&text), 0x7433_b0e4_7a4c_3c02);
    }

    fn populator(
        registry: &TerrainRegistry,
        terrain: i32,
        value: f32,
        weight: f32,
    ) -> Arc<dyn Populator> {
        Arc::new(TerrainPopulator::new(
            registry.get_by_id(terrain),
            Module::constant(0.0),
            Module::constant(value),
            weight,
        ))
    }

    #[test]
    fn region_selection_matches_engine_0_3_0() {
        let registry = TerrainRegistry::new();
        let selector = RegionSelector::new(vec![
            populator(&registry, 8, 0.1, 0.5),
            populator(&registry, 11, 0.2, 1.0),
            populator(&registry, 12, 0.3, 2.0),
        ]);
        let cases = [
            (0.0, 0x3dcccccd, 8),
            (0.1, 0x3e4ccccd, 11),
            (0.2, 0x3e4ccccd, 11),
            (0.4, 0x3e4ccccd, 11),
            (0.6, 0x3e99999a, 12),
            (0.8, 0x3e99999a, 12),
            (1.0, 0x3e99999a, 12),
        ];
        for (identity, value, terrain) in cases {
            let mut cell = Cell::new(&registry);
            cell.terrain_region_id = identity;
            selector.apply(&mut cell, 0.0, 0.0);
            assert_eq!(cell.value.to_bits(), value);
            assert_eq!(cell.terrain.id(), terrain);
        }

        let lerper = RegionLerper::new(
            populator(&registry, 11, 0.2, 1.0),
            populator(&registry, 12, 0.8, 1.0),
        );
        for (edge, value, terrain) in [
            (0.0, 0x3e4ccccd, 11),
            (0.25, 0x3eb33334, 12),
            (1.0, 0x3f4ccccd, 12),
        ] {
            let mut cell = Cell::new(&registry);
            cell.terrain_region_edge = edge;
            lerper.apply(&mut cell, 0.0, 0.0);
            assert_eq!(cell.value.to_bits(), value);
            assert_eq!(cell.terrain.id(), terrain);
        }

        let zeros = RegionSelector::new(vec![
            populator(&registry, 8, 0.4, 0.0),
            populator(&registry, 11, 0.6, 0.0),
        ]);
        for (identity, value, terrain) in [(0.0, 0x3ecccccd, 8), (1.0, 0x3f19999a, 11)] {
            let mut cell = Cell::new(&registry);
            cell.terrain_region_id = identity;
            zeros.apply(&mut cell, 0.0, 0.0);
            assert_eq!(cell.value.to_bits(), value);
            assert_eq!(cell.terrain.id(), terrain);
        }
    }
}
